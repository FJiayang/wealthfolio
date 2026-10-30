"""Exercise the actual candidate provenance step in disposable Git checkouts."""
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).parents[2]


def provenance_script():
    workflow = (ROOT / ".github/workflows/design-baseline.yml").read_text()
    step = workflow.split("      - name: Record source and environment\n", 1)[1]
    return textwrap.dedent(step.split("        run: |\n", 1)[1].split("      - name:", 1)[0])


class DesignProvenanceTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="design-provenance-")
        self.addCleanup(self.directory.cleanup)
        self.workspace = Path(self.directory.name) / "checkout with spaces $literal"
        self.workspace.mkdir()
        self.env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        subprocess.run(["git", "init", "-q", str(self.workspace)], env=self.env, check=True)
        subprocess.run([
            "git", "-C", str(self.workspace), "-c", "user.name=CI fixture",
            "-c", "user.email=ci@example.invalid", "-c", "commit.gpgsign=false",
            "commit", "--allow-empty", "-qm", "fixture",
        ], env=self.env, check=True)
        self.sha = subprocess.check_output(
            ["git", "-C", str(self.workspace), "rev-parse", "HEAD"], env=self.env, text=True,
        ).strip()
        self.env.update(
            GITHUB_WORKSPACE=str(self.workspace), REVISION=self.sha,
            PR_HEAD_SHA="a" * 40, PR_BASE_SHA="b" * 40,
            # Git's own test hook reproduces the container ownership mismatch
            # without chown, privileges, or changing any persistent trust config.
            GIT_TEST_ASSUME_DIFFERENT_OWNER="1",
        )

    def run_step(self, **overrides):
        env = {**self.env, **overrides}
        return subprocess.run(["sh", "-e", "-c", provenance_script()],
                              cwd=self.workspace, env=env, capture_output=True, text=True)

    def test_exact_checkout_is_trusted_for_one_read_only_command(self):
        untrusted = subprocess.run(["git", "rev-parse", "HEAD"], cwd=self.workspace,
                                   env=self.env, capture_output=True, text=True)
        self.assertNotEqual(untrusted.returncode, 0)
        self.assertIn("dubious ownership", untrusted.stderr)
        result = self.run_step()
        self.assertEqual(result.returncode, 0, result.stderr)
        record = (self.workspace / ".local/design-system/provenance.txt").read_text()
        self.assertIn(f"Source: {self.sha}\n", record)
        self.assertIn(f"PR head: {'a' * 40}\n", record)
        self.assertIn(f"PR base: {'b' * 40}\n", record)
        # The exception must not persist after the command completes.
        again = subprocess.run(["git", "rev-parse", "HEAD"], cwd=self.workspace,
                               env=self.env, capture_output=True, text=True)
        self.assertNotEqual(again.returncode, 0)
        self.assertIn("dubious ownership", again.stderr)

    def test_missing_or_wrong_workspace_fails_before_recording(self):
        for workspace in ("", str(self.workspace.parent), "relative-checkout"):
            with self.subTest(workspace=workspace):
                self.assertNotEqual(self.run_step(GITHUB_WORKSPACE=workspace).returncode, 0)
                self.assertFalse((self.workspace / ".local/design-system/provenance.txt").exists())
        env = dict(self.env)
        env.pop("GITHUB_WORKSPACE")
        result = subprocess.run(["sh", "-e", "-c", provenance_script()],
                                cwd=self.workspace, env=env, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)

    def test_wrong_or_missing_revision_never_records_provenance(self):
        for revision in ("0" * 40, "", "not-a-sha"):
            with self.subTest(revision=revision):
                self.assertNotEqual(self.run_step(REVISION=revision).returncode, 0)
                self.assertFalse((self.workspace / ".local/design-system/provenance.txt").exists())


if __name__ == "__main__":
    unittest.main()
