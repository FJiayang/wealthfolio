import json
import subprocess
import unittest
from pathlib import Path
from unittest.mock import patch

from fork_publish import Docker, image_identity, publish, version_key


IMAGE = "ghcr.io/fjiayang/wealthfolio"
SHA = "a" * 40
OLD_SHA = "b" * 40
NEW = "sha256:" + "1" * 64
OLD = "sha256:" + "2" * 64


def index(digest, marker):
    return {
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "digest": digest,
        "manifests": [
            {"digest": "sha256:" + marker * 63 + suffix,
             "platform": {"os": "linux", "architecture": arch}}
            for arch, suffix in (("amd64", "a"), ("arm64", "b"))
        ],
    }


class FakeDocker:
    def __init__(self, latest_version="3.11.0.18", exact=None):
        self.latest = index(OLD, "2")
        self.candidate = index(NEW, "1")
        self.exact = exact
        self.created = []
        self.identities = {}
        for manifest, version, sha in (
            (self.latest, latest_version, OLD_SHA),
            (self.candidate, "3.11.0.19", SHA),
        ):
            for descriptor in manifest["manifests"]:
                self.identities[f"{IMAGE}@{descriptor['digest']}"] = {
                    "org.opencontainers.image.version": version,
                    "org.opencontainers.image.revision": sha,
                }

    def inspect(self, ref, missing=False):
        if ref.endswith(":latest"):
            if self.latest is None:
                raise RuntimeError("latest unavailable")
            return self.latest
        if ref.endswith("-index"):
            return self.candidate
        if not missing:
            raise AssertionError("Expected optional exact version lookup")
        return self.exact

    def labels(self, ref):
        return self.identities[ref]

    def create(self, tag, *sources):
        self.created.append((tag, sources))


class PublishTests(unittest.TestCase):
    def run_publish(self, docker):
        return publish(docker, IMAGE, "3.11.0.19", SHA, "ci-123")

    def test_version_order_is_numeric_across_all_components(self):
        self.assertGreater(version_key("3.11.0.19"), version_key("3.11.0.9"))
        self.assertGreater(version_key("3.12.0.1"), version_key("3.11.0.999"))
        self.assertGreater(version_key("4.0.0.1"), version_key("3.99.99.999"))
        for value in ("3.11.0.0", "3.11.0.019", "3.11.0.19-rc1", "3.11.0", None):
            with self.subTest(value=value), self.assertRaises(ValueError):
                version_key(value)

    def test_new_version_promotes_all_aliases_from_pinned_index(self):
        docker = FakeDocker()
        self.assertTrue(self.run_publish(docker))
        self.assertEqual(docker.created[0], (f"{IMAGE}:ci-123-index", (
            f"{IMAGE}:ci-123-amd64", f"{IMAGE}:ci-123-arm64")))
        self.assertEqual([tag for tag, _ in docker.created[1:]], [
            f"{IMAGE}:3.11.0.19", f"{IMAGE}:sha-{SHA}", f"{IMAGE}:latest"])
        self.assertTrue(all(sources == (f"{IMAGE}@{NEW}",) for _, sources in docker.created[1:]))

    def test_older_release_does_not_regress_latest(self):
        docker = FakeDocker(latest_version="3.12.0.1")
        self.assertFalse(self.run_publish(docker))
        self.assertNotIn(f"{IMAGE}:latest", [tag for tag, _ in docker.created])
        self.assertIn(f"{IMAGE}:3.11.0.19", [tag for tag, _ in docker.created])

    def test_exact_existing_identical_is_not_rewritten(self):
        docker = FakeDocker(exact=index(NEW, "1"))
        self.run_publish(docker)
        self.assertNotIn(f"{IMAGE}:3.11.0.19", [tag for tag, _ in docker.created])

    def test_exact_existing_different_blocks_all_release_aliases(self):
        docker = FakeDocker(exact=index(OLD, "2"))
        with self.assertRaisesRegex(ValueError, "Exact version already exists"):
            self.run_publish(docker)
        self.assertEqual(len(docker.created), 1)  # Staging index only.

    def test_same_version_latest_different_content_blocks(self):
        docker = FakeDocker(latest_version="3.11.0.19")
        with self.assertRaisesRegex(ValueError, "latest already has this version"):
            self.run_publish(docker)
        self.assertEqual(len(docker.created), 1)

    def test_identical_latest_retry_is_noop_for_latest(self):
        docker = FakeDocker(exact=index(NEW, "1"))
        docker.latest = docker.candidate
        self.assertFalse(self.run_publish(docker))
        self.assertNotIn(f"{IMAGE}:latest", [tag for tag, _ in docker.created])

    def test_unavailable_latest_fails_before_any_write(self):
        docker = FakeDocker()
        docker.latest = None
        with self.assertRaises(RuntimeError):
            self.run_publish(docker)
        self.assertEqual(docker.created, [])

    def test_candidate_wrong_commit_blocks_release_aliases(self):
        docker = FakeDocker()
        for descriptor in docker.candidate["manifests"]:
            docker.identities[f"{IMAGE}@{descriptor['digest']}"]["org.opencontainers.image.revision"] = OLD_SHA
        with self.assertRaisesRegex(ValueError, "Staging images"):
            self.run_publish(docker)
        self.assertEqual(len(docker.created), 1)

    def test_mixed_architecture_identity_and_missing_platform_rejected(self):
        docker = FakeDocker()
        ref = f"{IMAGE}@{docker.latest['manifests'][0]['digest']}"
        docker.identities[ref]["org.opencontainers.image.version"] = "3.11.0.17"
        with self.assertRaises(ValueError):
            image_identity(docker, IMAGE, docker.latest)
        docker.latest["manifests"].pop()
        with self.assertRaises(ValueError):
            image_identity(docker, IMAGE, docker.latest)


class DockerParsingTests(unittest.TestCase):
    @patch("fork_publish.subprocess.run")
    def test_manifest_inspection_parses_documented_buildx_json(self, run):
        manifest = index(NEW, "1")
        run.return_value = subprocess.CompletedProcess([], 0, json.dumps(manifest), "")
        self.assertEqual(Docker().inspect(f"{IMAGE}:latest"), manifest)
        self.assertEqual(run.call_args.args[0][-2:], ["--format", "{{json .Manifest}}"])

    @patch("fork_publish.subprocess.run")
    def test_config_labels_parse_actual_single_manifest_shape(self, run):
        labels = {"org.opencontainers.image.version": "3.11.0.18"}
        run.return_value = subprocess.CompletedProcess([], 0, json.dumps({
            "architecture": "amd64", "os": "linux", "config": {"Labels": labels}}), "")
        self.assertEqual(Docker().labels(f"{IMAGE}@{NEW}"), labels)

    @patch("fork_publish.subprocess.run")
    def test_only_explicit_exact_reference_not_found_allows_creation(self, run):
        ref = f"{IMAGE}:3.11.0.19"
        run.return_value = subprocess.CompletedProcess([], 1, "", f"ERROR: {ref}: not found\n")
        self.assertIsNone(Docker().inspect(ref, missing=True))
        with self.assertRaises(RuntimeError):
            Docker().inspect(ref)
        for message in ("ERROR: unauthorized", "ERROR: 429 Too Many Requests", "ERROR: request timed out",
                        "ERROR: 503 unavailable", "ERROR: some-other-ref: not found", "proxy not found"):
            run.return_value = subprocess.CompletedProcess([], 1, "", message)
            with self.subTest(message=message), self.assertRaises(RuntimeError):
                Docker().inspect(ref, missing=True)

    @patch("fork_publish.subprocess.run")
    def test_invalid_success_payload_rejected(self, run):
        for payload in ("not json", "{}", '{"digest":"wrong"}'):
            run.return_value = subprocess.CompletedProcess([], 0, payload, "")
            with self.subTest(payload=payload), self.assertRaises(ValueError):
                Docker().inspect(f"{IMAGE}:latest")


class WorkflowContractTests(unittest.TestCase):
    def test_publish_job_serializes_guard_and_keeps_upstream_path(self):
        workflow = (Path(__file__).resolve().parents[1] / "workflows/docker-publish.yml").read_text()
        publish_job = workflow.split("  publish:\n", 1)[1]
        self.assertIn("group: docker-image-publication\n      queue: max", publish_job)
        self.assertNotIn("cancel-in-progress: true", publish_job)
        self.assertIn("if: needs.release-policy.outputs.fork == 'true'", publish_job)
        self.assertIn("if: needs.release-policy.outputs.fork != 'true'", publish_job)
        self.assertIn("python3 .github/scripts/fork_publish.py", publish_job)
        self.assertIn("--revision \"$RELEASE_SHA\"", publish_job)
        self.assertIn("format('Publish {0}', github.ref_name)", " ".join(workflow.split()))
        self.assertIn("format('Validate {0}', github.ref_name)", " ".join(workflow.split()))


if __name__ == "__main__":
    unittest.main()
