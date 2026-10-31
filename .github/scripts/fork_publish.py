"""Publish tested fork images without replacing an exact version or regressing latest."""

import argparse
import json
import re
import subprocess

from fork_release import FORK_TAG


DIGEST = re.compile(r"sha256:[0-9a-f]{64}")
PLATFORMS = {("linux", "amd64"), ("linux", "arm64")}


def version_key(version):
    if not isinstance(version, str) or not FORK_TAG.fullmatch("v" + version):
        raise ValueError(f"Invalid fork image version: {version!r}")
    return tuple(map(int, version.split(".")))


class Docker:
    def run(self, *args, missing=False):
        result = subprocess.run(
            ["docker", "buildx", "imagetools", *args], capture_output=True, text=True, check=False
        )
        if result.returncode:
            # Buildx reports resolver ErrNotFound this way. Never treat arbitrary
            # failures (authentication, throttling, transport) as an absent tag.
            reference = args[1] if args[0] == "inspect" else ""
            if missing and result.stderr.strip() == f"ERROR: {reference}: not found":
                return None
            raise RuntimeError(f"Docker {args[0]} failed: {result.stderr.strip()}")
        return result.stdout

    def inspect(self, reference, missing=False):
        raw = self.run("inspect", reference, "--format", "{{json .Manifest}}", missing=missing)
        if raw is None:
            return None
        manifest = json.loads(raw)
        if not DIGEST.fullmatch(manifest.get("digest", "")):
            raise ValueError(f"Missing image index digest for {reference}")
        return manifest

    def labels(self, reference):
        config = json.loads(self.run("inspect", reference, "--format", "{{json .Image}}"))
        return config["config"]["Labels"]

    def create(self, tag, *sources):
        self.run("create", "--tag", tag, *sources)


def image_identity(docker, image, manifest):
    """Read both image configurations through immutable platform digests."""
    identities = set()
    platforms = set()
    for descriptor in manifest.get("manifests", []):
        platform = descriptor.get("platform", {})
        key = (platform.get("os"), platform.get("architecture"))
        if key not in PLATFORMS:
            continue
        digest = descriptor.get("digest", "")
        if not DIGEST.fullmatch(digest):
            raise ValueError("Invalid platform manifest digest")
        labels = docker.labels(f"{image}@{digest}")
        version = labels.get("org.opencontainers.image.version")
        version_key(version)
        revision = labels.get("org.opencontainers.image.revision")
        if not isinstance(revision, str) or not re.fullmatch(r"[0-9a-f]{40}", revision):
            raise ValueError("Missing full image revision")
        identities.add((version, revision))
        platforms.add(key)
    if platforms != PLATFORMS or len(identities) != 1:
        raise ValueError("Expected consistent linux/amd64 and linux/arm64 image identities")
    return identities.pop()


def publish(docker, image, version, revision, staging_prefix):
    version_key(version)
    if not re.fullmatch(r"ghcr\.io/[a-z0-9_.-]+/wealthfolio", image):
        raise ValueError("Expected a fork Wealthfolio GHCR image")
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("Expected a full commit SHA")
    if not re.fullmatch(r"ci-[0-9]+", staging_prefix):
        raise ValueError("Expected the workflow run staging prefix")

    # The surrounding publish job serializes this whole read/compare/write
    # operation. Builds remain parallel. Missing latest is deliberately an error:
    # this existing fork already has a published baseline, not a bootstrap flow.
    latest = docker.inspect(f"{image}:latest")
    latest_version, _ = image_identity(docker, image, latest)

    candidate_tag = f"{image}:{staging_prefix}-index"
    docker.create(candidate_tag, f"{image}:{staging_prefix}-amd64", f"{image}:{staging_prefix}-arm64")
    candidate = docker.inspect(candidate_tag)
    if image_identity(docker, image, candidate) != (version, revision):
        raise ValueError("Staging images do not match the validated release identity")
    digest = candidate["digest"]
    source = f"{image}@{digest}"
    exact = docker.inspect(f"{image}:{version}", missing=True)
    if exact is not None and exact["digest"] != digest:
        raise ValueError("Exact version already exists with different content; refusing overwrite")
    if version_key(latest_version) == version_key(version) and latest["digest"] != digest:
        raise ValueError("latest already has this version with different content; refusing overwrite")

    if exact is None:
        docker.create(f"{image}:{version}", source)
    docker.create(f"{image}:sha-{revision}", source)
    update_latest = version_key(version) > version_key(latest_version)
    if update_latest:
        docker.create(f"{image}:latest", source)
    print(f"Published {image}:{version} at {digest}; latest updated: {str(update_latest).lower()}")
    return update_latest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--staging-prefix", required=True)
    args = parser.parse_args()
    try:
        publish(Docker(), args.image, args.version, args.revision, args.staging_prefix)
    except (ValueError, KeyError, TypeError, RuntimeError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
