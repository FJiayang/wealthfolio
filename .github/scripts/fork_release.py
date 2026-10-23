"""Validate release identity without putting four-part versions in SemVer manifests."""

import argparse
import json
import re
import tomllib
from pathlib import Path


UPSTREAM_REPOSITORY = "wealthfolio/wealthfolio"
NUMBER = r"(?:0|[1-9][0-9]*)"
BASE_VERSION = re.compile(rf"{NUMBER}\.{NUMBER}\.{NUMBER}")
FORK_TAG = re.compile(rf"v({NUMBER}\.{NUMBER}\.{NUMBER})\.([1-9][0-9]*)")


def release_version(root):
    config = json.loads((root / "fork-release.json").read_text())
    base = config["upstreamVersion"]
    revision = config["forkRevision"]
    if not isinstance(base, str) or not BASE_VERSION.fullmatch(base):
        raise ValueError("upstreamVersion must have three numeric components without leading zeros")
    if type(revision) is not int or revision < 1:
        raise ValueError("forkRevision must be a positive integer")
    if not re.fullmatch(r"[0-9a-f]{40}", config["upstreamCommit"]):
        raise ValueError("upstreamCommit must be a full lowercase commit SHA")
    for name in ("package.json", "apps/frontend/package.json", "apps/tauri/tauri.conf.json"):
        version = json.loads((root / name).read_text())["version"]
        if version != base:
            raise ValueError(f"{name} version {version!r} does not match upstreamVersion {base!r}")
    cargo = tomllib.loads((root / "Cargo.toml").read_text())
    if cargo["workspace"]["package"]["version"] != base:
        raise ValueError("Cargo.toml workspace version does not match upstreamVersion")
    return f"{base}.{revision}"


def release_policy(root, repository, ref, event, publish=False):
    is_fork = repository.lower() != UPSTREAM_REPOSITORY
    version = release_version(root) if is_fork else ""
    requested = event == "push" or (event == "workflow_dispatch" and publish)
    is_tag = ref.startswith("refs/tags/v")
    if event == "workflow_dispatch" and publish and not is_tag:
        raise ValueError("Publishing requires dispatching on a version tag, not a branch")
    if is_fork and is_tag:
        tag = ref.removeprefix("refs/tags/")
        if not FORK_TAG.fullmatch(tag):
            raise ValueError("Fork release tags must use vMAJOR.MINOR.PATCH.REVISION (revision > 0)")
        if tag != f"v{version}":
            raise ValueError(f"Tag {tag!r} does not match fork-release.json: v{version}")
    return {"fork": str(is_fork).lower(), "publish": str(requested and is_tag).lower(), "version": version}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--repository", required=True)
    parser.add_argument("--ref", required=True)
    parser.add_argument("--event", required=True)
    parser.add_argument("--publish", action="store_true")
    args = parser.parse_args()
    try:
        outputs = release_policy(args.root, args.repository, args.ref, args.event, args.publish)
    except (ValueError, KeyError, TypeError, OSError) as error:
        parser.error(str(error))
    for key, value in outputs.items():
        print(f"{key}={value}")


if __name__ == "__main__":
    main()
