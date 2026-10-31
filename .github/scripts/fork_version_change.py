"""Validate the reviewed release manifest against the current PR base."""
import argparse
import json
import re
import subprocess
from pathlib import Path

from fork_release import release_version

SHA = re.compile(r"[0-9a-f]{40}")


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def manifest_at(root, sha):
    if not SHA.fullmatch(sha):
        raise ValueError("Expected a full commit SHA")
    return json.loads(git(root, "show", f"{sha}:fork-release.json"))


def release_change(before, after):
    """Unchanged manifests are explicitly non-releasing changes."""
    if before == after:
        return False
    old_base, new_base = before["upstreamVersion"], after["upstreamVersion"]
    if old_base == new_base:
        if after["forkRevision"] != before["forkRevision"] + 1:
            raise ValueError("A release on the same upstream version must increment forkRevision by exactly 1")
    else:
        if tuple(map(int, new_base.split("."))) <= tuple(map(int, old_base.split("."))):
            raise ValueError("The upstream application version must increase")
        if after["forkRevision"] != 1:
            raise ValueError("A new upstream application version must start at forkRevision 1")
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    release_version(args.root)
    current = json.loads((args.root / "fork-release.json").read_text())
    changed = release_change(manifest_at(args.root, args.base), current)
    print(f"Release manifest changed: {str(changed).lower()}")


if __name__ == "__main__":
    main()
