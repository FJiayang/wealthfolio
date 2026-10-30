# Fork releases and upstream synchronization

This repository is FJiayang's fork of
[Wealthfolio](https://github.com/wealthfolio/wealthfolio). Upstream owns the
three-component application version; this fork appends a positive revision for
its Git release tags and Docker image tags.

For supported functionality and changes by version, see the
[brief fork release notes](fork-release-notes.md).

## Version contract

- Upstream application version: `3.11.0`.
- Fork release tag: `v3.11.0.1`; exact Docker image tag:
  `ghcr.io/fjiayang/wealthfolio:3.11.0.1`.
- `fork-release.json` records the upstream version, fork revision, and exact
  upstream commit incorporated into the fork. The revision is reserved for the
  next release; editing this file does not itself publish anything.
- Keep `package.json`, `apps/frontend/package.json`, `Cargo.toml`, and
  `apps/tauri/tauri.conf.json` at the upstream version. Four-component release
  names are not SemVer and must not be copied into these manifests or
  Cargo.lock.
- SDK and UI package versions and the version displayed in the application are
  also managed by upstream. Do not bump them for fork-only features; identify
  those capabilities by the fork release tag instead.
- Published fork tags use `vMAJOR.MINOR.PATCH.REVISION`; no leading zeros, zero
  revisions, prerelease suffixes, or reused tags. The tag must exactly match the
  release manifest at that commit.
- Fork Docker releases publish the full four-component version, a commit SHA
  alias, and `latest`. They do not publish upstream-shaped `3.11` or `3.11.0`
  aliases. Prefer an exact version or image digest for reproducible deployments.
  `latest` is mutable; the automatic publication guard prevents older versions
  from moving it backwards.

The current upstream base is commit `3fb05af985b3039ee875065a736cdafd8fc3bd45`,
integrated through the localization branch after the stable `v3.11.0` release.
Application manifests remain at 3.11.0. Upstream synchronization uses a real
merge, retaining the fork's history and compatible fork-only features. When
behavior is incompatible, upstream takes precedence. The two contributed PR
branches are merged separately without modifying or closing the upstream PRs. A
newer upstream release does not change this fork's base until deliberately
integrated.

## Automatic releases after a main merge

1. Start a feature branch from the latest fork `main`. A releasing PR increments
   `forkRevision` by exactly one. If the upstream application version increases,
   reset the revision to 1 and update `upstreamCommit` and application
   manifests. Merely syncing more upstream commits does not reset the revision.
   An unchanged `fork-release.json` explicitly means **no release**;
   documentation-only changes can use this path. A metadata change without the
   required version transition fails the PR's required `Build Status` through
   the CI helper job.
2. Run the related helper tests locally and let full PR CI verify the final
   merge revision. Do not write a second version-bump commit after merging.
3. `Automatic fork release` runs on a push to `main`. It selects the exact event
   commit, requires an associated merged PR into this fork's `main`, and
   verifies successful `PR Check` / `Build Status` for the PR head, base and
   resulting tree. It uses the mandatory reusable browser workflow's recorded
   merge SHA, not just the PR head SHA. Missing or stale evidence fails closed.
   Keep the existing strict branch protection and native auto-merge process.
4. The handler reads the committed release manifest and creates its exact
   lightweight Git tag. A same-name tag on the same commit is a safe retry; a
   different target fails without moving or deleting anything. The manifest and
   publication scripts are already present in the tagged revision, including
   this initial rollout PR. The handler never writes to `main`.
5. Creating a tag with `GITHUB_TOKEN` does not trigger another workflow's `push`
   event. The handler explicitly dispatches `docker-publish.yml` on that tag
   with `publish=true`. Its token uses `contents: write`, `actions: write`, and
   read access to PR/check metadata; no PAT or stored credential is needed. The
   Docker workflow keeps its existing `packages: write` permission. Automatic
   handling is restricted to `FJiayang/wealthfolio`, not upstream or other
   forks.
6. Both architecture builds and encryption smoke tests must succeed before
   publication. Builds can run concurrently; the publication job queues rather
   than cancelling releases. It inspects the already-published `latest` image's
   actual version, compares all four numeric components, and only advances
   `latest` to a newer version. An older release may publish its exact tag and
   SHA alias without rolling back `latest`. Both platform image labels must
   agree with the release manifest and commit. Registry inspection errors fail
   closed; this existing fork requires a previously published `latest`
   (currently established by .18), not an implicit first-release bootstrap.
7. Exact image versions are immutable too. An existing version with different
   content fails instead of being overwritten. Publication uses a run-local
   staging index and copies aliases from its pinned digest. If a partial publish
   fails, rerun the failed publish job against those same staging images. Do not
   rebuild and overwrite an already published version with different bytes.
8. A Git tag alone is not a successful release: verify the Docker workflow and
   the exact GHCR version's `linux/amd64` and `linux/arm64` manifests.

### Retry and trigger behavior

- Rerun a failed automatic-tag job or use **Automatic fork release → Run
  workflow**, branch `main`, with the original full merged `merge_sha`. The
  selected commit must remain an ancestor of `main`. This does not manufacture a
  new version or tag an arbitrary feature branch.
- Existing successful or running publishing runs for the same tag/commit are not
  dispatched again. Manual build-only runs do not count as published.
- If tag creation succeeds but dispatch fails, a rerun verifies the tag and
  dispatches it. A failed build keeps its tag; fix the cause and retry that
  version, or submit a new reviewed version if source changes are needed.
- A network error during a write is not automatically retried. Inspect the
  remote tag/run before rerunning. No workflow moves or deletes a release tag.
- Human merges and GitHub native auto-merge use the normal `main` event. If a
  future workflow itself merges with `GITHUB_TOKEN`, GitHub's recursive-event
  suppression can also suppress `main` events; that workflow must explicitly
  dispatch this handler. Do not add a PAT to work around this rule.
- GitHub's queued concurrency preserves pending requests but does not guarantee
  version order, so the numeric `latest` guard is still required. A manual
  release from an older revision that predates this guard must not be used after
  rollout; its historical workflow cannot enforce the new serialization policy.

Manual Docker runs remain build/test-only by default. Publishing requires both
an explicitly enabled `publish` input and a valid version tag. A publishing run
on a branch fails before the build. Legacy three-component fork tags are
rejected. Upstream repository runs retain their SemVer generation.

When integrating upstream, preserve upstream tags unchanged and update the exact
`upstreamCommit`. Never force-fetch upstream tags over local fork tags. A newer
upstream release does not change the fork's base until deliberately integrated.

Architecture impact: release workflow sequencing and publication metadata only.
Runtime networking, persistence, provider settings, encryption and application
version semantics remain unchanged. No new storage, service, PAT, main-branch
write or protection bypass is introduced.

## Historical tag migration

The historical fork names used upstream's patch-number space. Preserve the
following commit identities while renaming them:

| Legacy fork tag | Corrected fork tag | Commit                                     |
| --------------- | ------------------ | ------------------------------------------ |
| `v3.10.1`       | `v3.10.0.1`        | `cc5c67a799c91e04892e890b83d97b80679ffcdc` |
| `v3.10.2`       | `v3.10.0.2`        | `899b3ec9e2401fe00c71547e1fe772a544f666dd` |
| `v3.10.3`       | `v3.10.0.3`        | `a36692dbcabb2f738cab13804dbbf28d8c7e8c31` |
| `v3.10.4`       | `v3.10.0.4`        | `5747a25e57e7ed46bf12d1fcde8dcb844d9c7991` |
| `v3.10.5`       | `v3.10.0.5`        | `d5a08d5c1e2b5b0a6e5b9b339a20135c2c726dab` |
| `v3.10.6`       | `v3.10.0.6`        | `483aa3e260e2b62e8c68d715dc71cf97dd0bc0a4` |
| `v3.10.7`       | `v3.10.0.7`        | `a23a3d8b79b6ae40723c5c0487c17d66c299669c` |

These are historical aliases, not newly built releases. Their old commits do not
contain the new validator or four-component Docker tagging support. Creating
them through the GitHub API can still trigger workflows; it is not a safe way to
suppress builds. Before creating historical refs, record and explicitly disable
the relevant tag-triggered workflows, check for in-flight runs that could
publish, then create and verify each new ref's peeled commit. Restore each
workflow's original enabled/disabled state afterward, even if the migration
fails. Delete the conflicting legacy Git tag names only after explicit approval
and verification of the complete mapping; retain a record of original annotated
tag objects as well as commits. Never alter upstream's repository.

Do not dispatch builds on these historical aliases. If equivalent four-part
Docker tags are needed, retag verified existing image digests without rebuilding
or moving `latest`; verify both architectures and preserve existing deployment
references. Renaming a Git tag does not rename GHCR images. Do not remove legacy
image tags or release assets as a side effect of this migration. Check for
GitHub Releases and consumers of old archive/download URLs before deleting Git
refs, because those links can break.

## Application version and update limitations

The in-app version remains the upstream SemVer, so it does not identify the fork
revision. Use the Git tag, GHCR image tag/digest, and image version label to
identify fork builds. Existing web update checks still query the upstream update
service and cannot discover fork revisions. Native updater endpoints, public
keys, and signing identities also remain upstream's. Automatic signed native
releases are disabled for fork tags; manual native workflows still require
appropriate credentials. This policy does not establish a fork-native update
channel, change signing identity, or change data/storage compatibility.

Architecture impact: only release validation, image publication metadata, and
maintenance documentation change. Runtime networking, persistence, provider
settings, encryption, and application version semantics remain unchanged.
