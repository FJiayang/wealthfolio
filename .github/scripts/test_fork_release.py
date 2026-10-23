import json
import tempfile
import unittest
from pathlib import Path

from fork_release import release_policy, release_version


class ForkReleaseTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.config = {
            "upstreamVersion": "3.10.0",
            "forkRevision": 8,
            "upstreamCommit": "c77ae693d8a5930e3ef13ec001e4d6775d85be4a",
        }
        self.write_config()
        for name in ("package.json", "apps/frontend/package.json", "apps/tauri/tauri.conf.json"):
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps({"version": "3.10.0"}))
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion = "3.10.0"\n')

    def write_config(self):
        (self.root / "fork-release.json").write_text(json.dumps(self.config))

    def policy(self, ref="refs/tags/v3.10.0.8", event="push", publish=False):
        return release_policy(self.root, "FJiayang/wealthfolio", ref, event, publish)

    def test_exact_fork_release(self):
        self.assertEqual(self.policy(), {"fork": "true", "publish": "true", "version": "3.10.0.8"})

    def test_new_upstream_version_resets_fork_revision(self):
        self.config.update(upstreamVersion="3.11.0", forkRevision=1,
                           upstreamCommit="d1fda0b65f69d8557b1ef80e555c799215ad28b3")
        self.write_config()
        for name in ("package.json", "apps/frontend/package.json", "apps/tauri/tauri.conf.json", "Cargo.toml"):
            path = self.root / name
            path.write_text(path.read_text().replace("3.10.0", "3.11.0"))
        self.assertEqual(self.policy(ref="refs/tags/v3.11.0.1"),
                         {"fork": "true", "publish": "true", "version": "3.11.0.1"})
        with self.assertRaises(ValueError):
            self.policy(ref="refs/tags/v3.10.0.8")

    def test_reject_invalid_or_mismatched_tag(self):
        for tag in ("v3.10.8", "v3.10.0.0", "v3.10.0.08", "v03.10.0.8", "v3.010.0.8",
                    "v3.10.00.8", "v3.10.0.-1", "v3.10.0.8-beta", "v3.10.0.8.1",
                    "v3.11.0.8", "v3.10.0.7", "v3.10.0.9", "v3.10.0.8\n"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                self.policy(ref=f"refs/tags/{tag}")

    def test_manual_build_and_publish(self):
        self.assertEqual(self.policy(event="workflow_dispatch")["publish"], "false")
        self.assertEqual(self.policy(event="workflow_dispatch", publish=True)["publish"], "true")
        self.assertEqual(self.policy(ref="refs/heads/main", event="workflow_dispatch")["publish"], "false")
        with self.assertRaisesRegex(ValueError, "not a branch"):
            self.policy(ref="refs/heads/main", event="workflow_dispatch", publish=True)
        with self.assertRaises(ValueError):
            self.policy(ref="refs/tags/v3.10.7", event="workflow_dispatch", publish=True)

    def test_pull_request_does_not_publish(self):
        self.assertEqual(self.policy(ref="refs/pull/12/merge", event="pull_request")["publish"], "false")
        self.assertEqual(self.policy(event="pull_request", publish=True)["publish"], "false")

    def test_branch_push_does_not_publish(self):
        self.assertEqual(self.policy(ref="refs/heads/main")["publish"], "false")

    def test_all_application_versions_must_match_base(self):
        for name in ("package.json", "apps/frontend/package.json", "apps/tauri/tauri.conf.json", "Cargo.toml"):
            path = self.root / name
            original = path.read_text()
            with self.subTest(name=name):
                path.write_text(original.replace("3.10.0", "3.11.0"))
                with self.assertRaises(ValueError):
                    release_version(self.root)
                path.write_text(original)

    def test_invalid_config(self):
        for key, value in (("forkRevision", 0), ("forkRevision", -1), ("forkRevision", "08"),
                           ("forkRevision", True), ("forkRevision", 1.5),
                           ("upstreamVersion", "03.10.0"), ("upstreamVersion", "3.10.0.1"),
                           ("upstreamCommit", "c77ae69")):
            original = self.config[key]
            with self.subTest(key=key, value=value):
                self.config[key] = value
                self.write_config()
                with self.assertRaises(ValueError):
                    release_version(self.root)
                self.config[key] = original
                self.write_config()

    def test_upstream_retains_three_part_policy_without_fork_metadata(self):
        (self.root / "fork-release.json").unlink()
        output = release_policy(self.root, "wealthfolio/wealthfolio", "refs/tags/v3.11.0", "push")
        self.assertEqual(output, {"fork": "false", "publish": "true", "version": ""})
        output = release_policy(self.root, "wealthfolio/wealthfolio", "refs/heads/main", "workflow_dispatch")
        self.assertEqual(output["publish"], "false")


if __name__ == "__main__":
    unittest.main()
