import unittest
import json
import tempfile
from pathlib import Path
from unittest.mock import patch

from auto_fork_release import dispatch_needed, ensure_tag, merged_pr, tested_revision, verify_ci, release, GitHub
from fork_version_change import release_change

SHA = "a" * 40
HEAD = "b" * 40
BASE = "c" * 40
TREE = "d" * 40
TESTED = "e" * 40
TAG = "v3.11.0.19"
REPO = "FJiayang/wealthfolio"


def pr():
    return {"number": 2, "merged_at": "2026-10-30T00:00:00Z", "merge_commit_sha": SHA,
            "base": {"ref": "main", "repo": {"full_name": REPO}}, "head": {"sha": HEAD}}


def run():
    return {"id": 123, "event": "pull_request", "status": "completed", "conclusion": "success",
            "path": ".github/workflows/pr-check.yml", "head_sha": HEAD,
            "referenced_workflows": [{"path": f"{REPO}/.github/workflows/browser-e2e.yml@{TESTED}",
                                      "sha": TESTED, "ref": "refs/pull/2/merge"}]}


class FakeAPI:
    def __init__(self, values):
        self.values = values
        self.writes = []

    def request(self, path, data=None, **kwargs):
        if data is not None:
            self.writes.append((path, data))
        value = self.values[path]
        if isinstance(value, Exception):
            raise value
        if callable(value):
            return value()
        return value

    def pages(self, path, key=None):
        return iter(self.values[path])


class VersionTransitionTests(unittest.TestCase):
    def setUp(self):
        self.before = {"upstreamVersion": "3.11.0", "forkRevision": 18, "upstreamCommit": BASE}

    def test_unchanged_is_not_a_release(self):
        self.assertFalse(release_change(self.before, dict(self.before)))

    def test_same_upstream_increments_exactly_once(self):
        self.assertTrue(release_change(self.before, self.before | {"forkRevision": 19}))
        for revision in (1, 17, 20):
            with self.subTest(revision=revision), self.assertRaises(ValueError):
                release_change(self.before, self.before | {"forkRevision": revision})

    def test_upstream_commit_change_still_needs_revision(self):
        with self.assertRaises(ValueError):
            release_change(self.before, self.before | {"upstreamCommit": HEAD})
        self.assertTrue(release_change(self.before, self.before | {"upstreamCommit": HEAD, "forkRevision": 19}))

    def test_new_upstream_resets_to_one(self):
        self.assertTrue(release_change(self.before, self.before | {"upstreamVersion": "3.12.0", "forkRevision": 1}))
        for version, revision in (("3.12.0", 19), ("3.9.0", 1)):
            with self.subTest(version=version), self.assertRaises(ValueError):
                release_change(self.before, self.before | {"upstreamVersion": version, "forkRevision": revision})


class ReleaseProvenanceTests(unittest.TestCase):
    def test_exactly_one_merged_main_pr(self):
        self.assertEqual(merged_pr([pr()], SHA)["number"], 2)
        for pulls in ([], [pr(), pr()], [pr() | {"merged_at": None}], [pr() | {"merge_commit_sha": HEAD}],
                      [pr() | {"base": {"ref": "develop"}}]):
            with self.subTest(pulls=pulls), self.assertRaises(ValueError):
                merged_pr(pulls, SHA)

    def test_tested_merge_sha_not_just_head(self):
        self.assertEqual(tested_revision(run(), pr()), TESTED)
        for change in ({"head_sha": SHA}, {"conclusion": "failure"}, {"status": "in_progress"},
                       {"path": "different.yml"}, {"event": "push"}, {"referenced_workflows": []}):
            with self.subTest(change=change):
                self.assertIsNone(tested_revision(run() | change, pr()))

    def test_reusable_workflow_must_be_this_pr_and_repository(self):
        for ref in ("refs/pull/3/merge", "refs/heads/main"):
            candidate = run()
            candidate["referenced_workflows"][0]["ref"] = ref
            self.assertIsNone(tested_revision(candidate, pr()))
        candidate = run()
        candidate["referenced_workflows"][0]["path"] = f"untrusted/repo/browser-e2e.yml@{TESTED}"
        self.assertIsNone(tested_revision(candidate, pr()))

    def ci_api(self, commit=None, gates=None):
        return FakeAPI({f"actions/workflows/pr-check.yml/runs?event=pull_request&head_sha={HEAD}": [run()],
                        "actions/runs/123": run(),
                        f"git/commits/{TESTED}": commit or {"tree": {"sha": TREE}, "parents": [{"sha": BASE}, {"sha": HEAD}]},
                        "actions/runs/123/jobs": gates if gates is not None else [{"name": "Build Status", "conclusion": "success"}]})

    def test_success_requires_tree_base_and_head(self):
        target = {"tree": {"sha": TREE}, "parents": [{"sha": BASE}]}
        self.assertEqual(verify_ci(self.ci_api(), pr(), target), 123)
        for commit in ({"tree": {"sha": HEAD}, "parents": [{"sha": BASE}, {"sha": HEAD}]},
                       {"tree": {"sha": TREE}, "parents": [{"sha": SHA}, {"sha": HEAD}]},
                       {"tree": {"sha": TREE}, "parents": [{"sha": BASE}, {"sha": SHA}]}):
            with self.subTest(commit=commit), self.assertRaises(ValueError):
                verify_ci(self.ci_api(commit=commit), pr(), target)
        for gates in ([], [{"name": "Build Status", "conclusion": "skipped"}]):
            with self.subTest(gates=gates), self.assertRaises(ValueError):
                verify_ci(self.ci_api(gates=gates), pr(), target)


class TagAndDispatchTests(unittest.TestCase):
    def test_create_and_read_back_new_tag(self):
        values = iter([None, {"object": {"type": "commit", "sha": SHA}}])
        api = FakeAPI({f"git/ref/tags/{TAG}": lambda: next(values), "git/refs": {}})
        ensure_tag(api, TAG, SHA)
        self.assertEqual(api.writes, [("git/refs", {"ref": f"refs/tags/{TAG}", "sha": SHA})])

    def test_existing_lightweight_and_annotated_tags(self):
        for kind in ("commit", "tag"):
            api = FakeAPI({f"git/ref/tags/{TAG}": {"object": {"type": kind, "sha": SHA}},
                           f"git/tags/{SHA}": {"object": {"type": "commit", "sha": SHA}}})
            ensure_tag(api, TAG, SHA)
            self.assertEqual(api.writes, [])

    def test_conflicting_tag_is_never_changed(self):
        api = FakeAPI({f"git/ref/tags/{TAG}": {"object": {"type": "commit", "sha": HEAD}}})
        with self.assertRaises(ValueError):
            ensure_tag(api, TAG, SHA)
        self.assertEqual(api.writes, [])

    def test_uncertain_create_is_not_retried(self):
        api = FakeAPI({f"git/ref/tags/{TAG}": None, "git/refs": TimeoutError("uncertain write")})
        with self.assertRaises(TimeoutError):
            ensure_tag(api, TAG, SHA)
        self.assertEqual(len(api.writes), 1)

    def test_dispatch_is_idempotent_for_actual_publish_runs(self):
        base = {"head_branch": TAG, "head_sha": SHA, "event": "workflow_dispatch", "display_title": f"Publish {TAG}"}
        for state in ({"status": "queued"}, {"status": "in_progress"}, {"status": "completed", "conclusion": "success"}):
            with self.subTest(state=state):
                self.assertFalse(dispatch_needed([base | state], TAG, SHA))
        self.assertTrue(dispatch_needed([base | {"status": "completed", "conclusion": "failure"}], TAG, SHA))
        self.assertTrue(dispatch_needed([base | {"head_sha": HEAD, "status": "queued"}], TAG, SHA))
        self.assertTrue(dispatch_needed([base | {"display_title": f"Validate {TAG}", "status": "completed", "conclusion": "success"}], TAG, SHA))
        self.assertFalse(dispatch_needed([base | {"event": "push", "display_title": "old release", "status": "completed", "conclusion": "success"}], TAG, SHA))


class ReleaseFlowTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.before = {"upstreamVersion": "3.11.0", "forkRevision": 18, "upstreamCommit": BASE}
        self.current = self.before | {"forkRevision": 19}
        (self.root / "fork-release.json").write_text(json.dumps(self.current))
        self.api = FakeAPI({f"git/commits/{SHA}": {"tree": {"sha": TREE}, "parents": [{"sha": BASE}]},
                            f"commits/{SHA}/pulls": [pr()],
                            f"actions/workflows/docker-publish.yml/runs?head_sha={SHA}": [],
                            "actions/workflows/docker-publish.yml/dispatches": None})

    def execute(self, **options):
        with patch("auto_fork_release.git", return_value=SHA), \
             patch("auto_fork_release.subprocess.run") as ancestor, \
             patch("auto_fork_release.manifest_at", return_value=self.before), \
             patch("auto_fork_release.release_version", return_value="3.11.0.19"), \
             patch("auto_fork_release.verify_ci", **options) as gate, \
             patch("auto_fork_release.ensure_tag") as tag:
            release(self.api, self.root, SHA)
            return ancestor, gate, tag

    def test_tag_then_explicit_dispatch_on_version_ref(self):
        ancestor, gate, tag = self.execute(return_value=123)
        ancestor.assert_called_once()
        gate.assert_called_once()
        tag.assert_called_once_with(self.api, TAG, SHA)
        self.assertEqual(self.api.writes, [("actions/workflows/docker-publish.yml/dispatches",
                                           {"ref": TAG, "inputs": {"publish": True}})])

    def test_unchanged_manifest_never_creates_tag_or_dispatches(self):
        (self.root / "fork-release.json").write_text(json.dumps(self.before))
        _, gate, tag = self.execute()
        gate.assert_not_called()
        tag.assert_not_called()
        self.assertEqual(self.api.writes, [])

    def test_failed_gate_prevents_all_writes(self):
        with self.assertRaises(ValueError):
            self.execute(side_effect=ValueError("stale CI"))
        self.assertEqual(self.api.writes, [])

    def test_uncertain_dispatch_is_not_retried(self):
        self.api.values["actions/workflows/docker-publish.yml/dispatches"] = TimeoutError("uncertain")
        with self.assertRaises(TimeoutError):
            self.execute(return_value=123)
        self.assertEqual(len(self.api.writes), 1)


class APIAndWorkflowTests(unittest.TestCase):
    def test_pagination_reads_every_page(self):
        api = GitHub("test-not-a-real-token")
        with patch.object(api, "request", side_effect=[{"jobs": list(range(100))}, {"jobs": [100]}]) as request:
            self.assertEqual(list(api.pages("actions/runs/1/jobs", "jobs")), list(range(101)))
            self.assertEqual(request.call_count, 2)
            self.assertTrue(request.call_args.args[0].endswith("per_page=100&page=2"))

    def test_privileged_handler_only_checks_out_main_ancestors(self):
        root = Path(__file__).resolve().parents[1]
        workflow = (root / "workflows/auto-fork-release.yml").read_text()
        self.assertIn("branches: [main]", workflow)
        self.assertIn("github.ref == 'refs/heads/main'", workflow)
        self.assertNotIn("pull_request_target:", workflow)
        self.assertNotIn("pull_request:", workflow)
        self.assertIn("persist-credentials: false", workflow)
        self.assertIn('git merge-base --is-ancestor "$MERGE_SHA" origin/main', workflow)
        self.assertIn("queue: max", workflow)
        self.assertIn("actions: write", workflow)
        ci = (root / "workflows/pr-check.yml").read_text()
        self.assertIn('fork_version_change.py --base "$BASE_SHA"', ci)


if __name__ == "__main__":
    unittest.main()
