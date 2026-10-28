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
  `latest` is mutable; publishing an older release can move it backwards.

The current upstream base is commit `3fb05af985b3039ee875065a736cdafd8fc3bd45`,
integrated through the localization branch after the stable `v3.11.0` release.
Application manifests remain at 3.11.0. Upstream synchronization uses a real
merge, retaining the fork's history and compatible fork-only features. When
behavior is incompatible, upstream takes precedence. The two contributed PR
branches are merged separately without modifying or closing the upstream PRs. A
newer upstream release does not change this fork's base until deliberately
integrated.

## Publishing the next fork revision

1. Integrate and test changes on the fork. Increment `forkRevision` for each new
   release; the 3.11.0 upstream version resets this fork to revision 1.
2. Run
   `python3 -m unittest discover -s .github/scripts -p 'test_fork_release.py'`
   and validate the intended tag:
   `python3 .github/scripts/fork_release.py --repository FJiayang/wealthfolio --ref refs/tags/v3.11.0.1 --event push`.
3. After review, create the matching immutable release tag on the intended
   commit. Do not recreate or move a previously published tag.
4. The Docker workflow validates the identity before building. Both architecture
   images must pass encryption smoke tests before release manifests publish.
5. Verify the exact version tag and both architectures in GHCR before treating
   the release as available. A tag alone is not evidence of a successful build.

Manual Docker runs remain build/test-only by default. Publishing requires both
an explicitly enabled `publish` input and a valid version tag. A publishing run
on a branch fails before the build. Legacy three-component fork tags are
rejected by the new policy. Upstream repository runs retain their SemVer tag
generation.

When integrating upstream, preserve upstream tags unchanged, update the exact
`upstreamCommit`, and verify application manifests against `upstreamVersion`. If
the upstream application version changes, reset the fork revision to 1 for that
new base; if it does not, continue the current revision sequence. Do not reset
just because more upstream commits were integrated. Avoid force-fetching
upstream tags over local fork tags; inspect remote refs or use a separate
upstream tag namespace when resolving historical collisions.

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
