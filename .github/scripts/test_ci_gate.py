"""Regression tests for the required aggregate and full-coverage workflow contract."""
import copy
from pathlib import Path
import re
import unittest

from ci_gate import MANDATORY, MOBILE, failures

ROOT = Path(__file__).parents[2]


def passing():
    needs = {name: {"result": "success"} for name in MANDATORY | MOBILE.keys()}
    needs["changes"]["outputs"] = {"android": "true", "ios": "true"}
    return needs


class GateTests(unittest.TestCase):
    def test_complete_success(self):
        self.assertEqual(failures(passing()), [])

    def test_every_mandatory_job_fails_closed(self):
        for job in MANDATORY:
            for result in ("failure", "cancelled", "skipped", "neutral", ""):
                with self.subTest(job=job, result=result):
                    needs = passing()
                    needs[job]["result"] = result
                    self.assertTrue(failures(needs))
            needs = passing()
            del needs[job]
            self.assertTrue(failures(needs))

    def test_only_unrequested_mobile_compilation_may_skip(self):
        for job, platform in MOBILE.items():
            needs = passing()
            needs[job]["result"] = "skipped"
            self.assertTrue(failures(needs))
            needs["changes"]["outputs"][platform] = "false"
            self.assertEqual(failures(needs), [])
            for state in ("failure", "cancelled", "missing"):
                broken = copy.deepcopy(needs)
                broken[job]["result"] = state
                self.assertTrue(failures(broken))
            del needs["changes"]["outputs"][platform]
            self.assertTrue(failures(needs))

    def test_unknown_failed_dependency_is_not_ignored(self):
        needs = passing()
        needs["new-check"] = {"result": "failure"}
        self.assertTrue(failures(needs))


class WorkflowContractTests(unittest.TestCase):
    def test_required_aggregate_is_stable_and_covers_all_jobs(self):
        workflow = (ROOT / ".github/workflows/pr-check.yml").read_text()
        jobs = set(re.findall(r"^  ([\w-]+):$", workflow, re.MULTILINE))
        jobs -= {"pull_request", "merge_group", "contents"}
        self.assertEqual(jobs - {"build-status"}, MANDATORY | MOBILE.keys())
        gate = workflow.split("  build-status:\n")[1]
        self.assertIn("name: Build Status", gate)
        self.assertIn("if: always()", gate)
        self.assertIn("toJSON(needs)", gate)
        for job in MANDATORY | MOBILE.keys():
            self.assertRegex(gate, rf"\b{job},")
        self.assertNotIn("paths:", workflow)
        for condition in re.findall(r"^    if: (.+)$", workflow, re.MULTILINE):
            self.assertIn(condition, ["always()", "needs.changes.outputs.android == 'true'",
                                      "needs.changes.outputs.ios == 'true'"])

    def test_all_browser_suites_are_required_and_retry_free(self):
        workflow = (ROOT / ".github/workflows/browser-e2e.yml").read_text()
        self.assertIn("suite: [main, profiles]", workflow)
        self.assertIn("suite: [net-worth, addon-sandbox]", workflow)
        self.assertIn("browser: [chromium, webkit]", workflow)
        self.assertIn("chromium firefox webkit", workflow)
        self.assertIn("pnpm test:e2e --workers=1 --retries=0 --forbid-only", workflow)
        self.assertIn("cp .env.web.example .env.web", workflow)
        self.assertIn('export WF_SECRET_KEY="$(openssl rand -base64 32)"', workflow)
        self.assertIn("export WF_MCP_ENABLED=true", workflow)
        self.assertIn("--update-snapshots=none", workflow)
        self.assertNotIn("continue-on-error", workflow)
        self.assertNotRegex(workflow, r"--(?:grep|grep-invert|pass-with-no-tests)")
        self.assertIn("--trace=retain-on-failure", workflow)
        runner = (ROOT / "scripts/run-addon-sandbox-e2e.mjs").read_text()
        self.assertIn("...process.argv.slice(2)", runner)

    def test_snapshot_generation_is_not_the_acceptance_check(self):
        candidate = (ROOT / ".github/workflows/design-baseline.yml").read_text()
        browser = (ROOT / ".github/workflows/browser-e2e.yml").read_text()
        for workflow in (candidate, browser):
            self.assertIn("mcr.microsoft.com/playwright:v1.58.2-noble", workflow)
            self.assertIn('version !== "1.58.2"', workflow)
            self.assertIn("include-hidden-files: true", workflow)
        self.assertIn("--update-snapshots=all", candidate)
        self.assertNotIn("contents: write", candidate)
        self.assertNotIn("pull-requests: write", candidate)
        self.assertNotIn("download-artifact", browser)
        config = (ROOT / "playwright.design-system.ci.config.ts").read_text()
        self.assertIn('updateSnapshots: "none"', config)
        self.assertIn("reuseExistingServer: false", config)

    def test_cross_platform_security_suites_are_reused(self):
        for name in ("http-client-policy", "native-secret-store", "server-secret-store"):
            workflow = (ROOT / f".github/workflows/{name}.yml").read_text()
            self.assertIn("workflow_call:", workflow)
            self.assertNotIn("pull_request:", workflow)
            self.assertIn("os: [macos-latest, windows-latest, ubuntu-latest]", workflow)

    def test_pre_push_does_not_run_full_tests(self):
        hook = (ROOT / ".githooks/pre-push").read_text()
        self.assertNotIn("ci-check.sh", hook)
        self.assertNotIn("test:e2e", hook)
        self.assertNotIn("cargo test", hook)
        self.assertNotIn("pnpm test", hook)


if __name__ == "__main__":
    unittest.main()
