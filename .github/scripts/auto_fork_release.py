"""Tag a reviewed main merge and explicitly dispatch the existing Docker workflow."""
import json
import os
import subprocess
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

from fork_release import release_version
from fork_version_change import SHA, git, manifest_at, release_change

REPOSITORY = "FJiayang/wealthfolio"
WORKFLOW = "docker-publish.yml"


class GitHub:
    def __init__(self, token):
        self.token = token

    def request(self, path, data=None, method=None, missing_ok=False):
        request = urllib.request.Request(
            f"https://api.github.com/repos/{REPOSITORY}/{path}",
            data=None if data is None else json.dumps(data).encode(),
            headers={"Authorization": f"Bearer {self.token}", "Accept": "application/vnd.github+json",
                     "X-GitHub-Api-Version": "2022-11-28", "Content-Type": "application/json"},
            method=method,
        )
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                body = response.read()
                return json.loads(body) if body else None
        except urllib.error.HTTPError as error:
            if missing_ok and error.code == 404:
                return None
            # Do not retry uncertain writes or print response headers/token values.
            raise RuntimeError(f"GitHub {request.method} {path} returned {error.code}") from None

    def pages(self, path, key=None):
        separator = "&" if "?" in path else "?"
        page = 1
        while True:
            result = self.request(f"{path}{separator}per_page=100&page={page}")
            items = result[key] if key else result
            yield from items
            if len(items) < 100:
                return
            page += 1


def merged_pr(pulls, sha):
    matching = [pr for pr in pulls if pr.get("merged_at") and pr.get("merge_commit_sha") == sha
                and pr.get("base", {}).get("ref") == "main"
                and pr.get("base", {}).get("repo", {}).get("full_name") == REPOSITORY]
    if len(matching) != 1:
        raise ValueError("Expected exactly one merged main PR for the selected commit")
    return matching[0]


def tested_revision(run, pr):
    if (run.get("event") != "pull_request" or run.get("conclusion") != "success"
            or run.get("status") != "completed" or run.get("head_sha") != pr["head"]["sha"]
            or run.get("path") != ".github/workflows/pr-check.yml"):
        return None
    prefix = f"{REPOSITORY}/.github/workflows/browser-e2e.yml@"
    refs = [item for item in run.get("referenced_workflows", [])
            if item.get("path", "").startswith(prefix)
            and item.get("ref") == f"refs/pull/{pr['number']}/merge"]
    if len(refs) != 1 or not SHA.fullmatch(refs[0].get("sha", "")):
        return None
    return refs[0]["sha"]


def verify_ci(api, pr, target):
    # The mandatory reusable browser workflow records the actual merge-ref SHA,
    # unlike run.head_sha, which is only the PR head. This binds CI to the base
    # and resulting tree without adding a separate receipt/artifact mechanism.
    query = urllib.parse.urlencode({"event": "pull_request", "head_sha": pr["head"]["sha"]})
    runs = api.pages(f"actions/workflows/pr-check.yml/runs?{query}", "workflow_runs")
    for summary in runs:
        run = api.request(f"actions/runs/{summary['id']}")
        tested = tested_revision(run, pr)
        if not tested:
            continue
        commit = api.request(f"git/commits/{tested}")
        parents = [item["sha"] for item in commit["parents"]]
        if (commit["tree"]["sha"] != target["tree"]["sha"] or len(parents) != 2
                or parents[0] != target["parents"][0]["sha"] or parents[1] != pr["head"]["sha"]):
            continue
        jobs = api.pages(f"actions/runs/{run['id']}/jobs", "jobs")
        gates = [job for job in jobs if job["name"] == "Build Status"]
        if len(gates) == 1 and gates[0].get("conclusion") == "success":
            return run["id"]
    raise ValueError("No successful Build Status verifies this PR head, base, and merged tree")


def ensure_tag(api, tag, sha):
    path = f"git/ref/tags/{tag}"
    ref = api.request(path, missing_ok=True)
    if ref is None:
        api.request("git/refs", {"ref": f"refs/tags/{tag}", "sha": sha})
        ref = api.request(path)
    obj = ref["object"]
    while obj["type"] == "tag":
        obj = api.request(f"git/tags/{obj['sha']}")["object"]
    if obj["type"] != "commit" or obj["sha"] != sha:
        raise ValueError(f"Existing {tag} points elsewhere; never move a published tag")


def dispatch_needed(runs, tag, sha):
    matches = [run for run in runs if run.get("head_branch") == tag and run.get("head_sha") == sha
               and (run.get("event") == "push" or (run.get("event") == "workflow_dispatch"
                    and run.get("display_title") == f"Publish {tag}"))]
    if any(run.get("conclusion") == "success" for run in matches):
        return False
    if any(run.get("status") != "completed" for run in matches):
        return False
    return True


def release(api, root, sha):
    if not SHA.fullmatch(sha):
        raise ValueError("Expected a full merge commit SHA")
    if git(root, "rev-parse", "HEAD") != sha:
        raise ValueError("Checkout must match the selected merge commit")
    # A manual retry may select an older main ancestor, never an arbitrary branch.
    subprocess.run(["git", "-C", str(root), "merge-base", "--is-ancestor", sha, "origin/main"], check=True)
    target = api.request(f"git/commits/{sha}")
    if not target["parents"]:
        raise ValueError("Release commit has no main parent")
    before = manifest_at(root, target["parents"][0]["sha"])
    current = json.loads((root / "fork-release.json").read_text())
    version = release_version(root)
    pr = merged_pr(list(api.pages(f"commits/{sha}/pulls")), sha)
    if not release_change(before, current):
        print("Non-release PR: manifest unchanged; no tag or publication")
        return
    run_id = verify_ci(api, pr, target)
    tag = f"v{version}"
    ensure_tag(api, tag, sha)
    query = urllib.parse.urlencode({"head_sha": sha})
    runs = api.pages(f"actions/workflows/{WORKFLOW}/runs?{query}", "workflow_runs")
    if dispatch_needed(runs, tag, sha):
        api.request(f"actions/workflows/{WORKFLOW}/dispatches",
                    {"ref": tag, "inputs": {"publish": True}})
        print(f"Created/verified {tag} at {sha}; dispatched Docker after PR Check {run_id}")
    else:
        print(f"Verified {tag}; Docker is already running or succeeded")


def main():
    if os.environ.get("GITHUB_REPOSITORY") != REPOSITORY:
        raise ValueError("Automatic publication is restricted to the maintained fork")
    release(GitHub(os.environ["GH_TOKEN"]), Path.cwd(), os.environ["MERGE_SHA"])


if __name__ == "__main__":
    main()
