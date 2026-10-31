# Development workflow

## From latest main to a checked PR

Start from the latest fork `main`, not a stale local branch:

```bash
git fetch origin main
git switch -c feature/<short-name> origin/main
```

Implement the change, add a regression test, and run the relevant local unit
tests. Use one-shot Vitest files or a focused Rust crate/test filter; add
targeted lint/type/static checks appropriate to the change. Record the actual
commands and results in the PR. Documentation/CI-only changes need their helper
tests and static validation, not local application/browser suites.

After focused tests pass, commit and push, then create a PR against `main`. CI
owns the full verification. A draft still receives CI; mark it ready only when
implementation and review preparation are complete. Do not interpret a local
focused pass as a full-suite pass.

## Required CI

`PR Check` runs on every PR, with no path-filter for tests. It tests GitHub's PR
merge revision (the proposed head plus base), and records the merge/head/base
SHAs in the `Build Status` summary. It also supports `merge_group` if a merge
queue is introduced later.

| Check                                | Coverage                                                                                                     |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| CI helpers                           | Python workflow/release/helper regression tests                                                              |
| Translation parity                   | Checker tests and every locale/namespace                                                                     |
| Design ratchet and formatting        | Existing design debt limits and repository formatting                                                        |
| Frontend Check                       | All script tests, themes, lint, package types, web and Tauri frontend builds                                 |
| Frontend Tests                       | All frontend Vitest tests, two shards                                                                        |
| Rust Check                           | Format, workspace all-feature Clippy, workspace tests, AI fixture tests, server release check                |
| HTTP / native / server secret stores | Existing compatibility suites on Linux, macOS, Windows, including native credential-store tests              |
| Browser E2E: main                    | Entire serial fixture-backed application suite; Chrome plus the existing WebKit mobile case                  |
| Browser E2E: profiles                | Isolated profile lifecycle suite against the production web build                                            |
| Browser E2E: net worth               | Complete independent layout/RTL harness                                                                      |
| Browser E2E: addon sandbox           | Complete Chromium, Firefox, WebKit isolation suite                                                           |
| Browser E2E: design system           | Complete Chromium/WebKit functional, visual, RTL, contrast and accessibility suite, three shards per browser |

Android/iOS **compilation**, not tests, keeps the existing changed-file filter
to avoid redundant SDK builds for documentation/frontend-only changes. Shared
backend/config changes still request the affected targets. Native compatibility
**tests** remain mandatory on all three desktop operating systems; iOS device
and simulator compilation is owned by the conditional iOS job, not duplicated
inside the native-store suite.

The stable required check is **Build Status**, from **GitHub Actions**. It fails
on any required job failure, cancellation, missing result, or unexpected skip.
Only explicitly unrequested mobile compilation may skip. Test matrices use
`fail-fast: false` to retain other results. Browser tests use zero retries and
reject focused `test.only`; no `grep` exclusions or screenshot updates are used
by acceptance jobs. Existing opt-in external-provider tests remain opt-in; CI's
full deterministic suite is not a claim that live third-party services or
physical mobile hardware were tested.

Browser jobs upload HTML reports, failure traces and actual/expected/diff images
for 14 days. Jobs use independent databases, bounded timeouts, and cancellation
of obsolete PR runs. Diagnose a failing assertion or environment error before
rerunning; do not increase retries, weaken expectations, skip a test, or replace
screenshots simply to obtain green CI. Product fixes outside the current task's
scope need separate authorization.

## Native auto-merge prerequisites

Repository settings are an owner/admin action, separate from a code change:

- Enable **Allow auto-merge**.
- Protect `main`: require a PR and **Build Status** from GitHub Actions, require
  the branch to be up to date, prevent force pushes/deletion, and apply the
  rules without an administrator bypass. Keep merge commits enabled for upstream
  synchronization.
- Respect any additional configured review/conversation requirements; never
  self-approve or circumvent them. CI does not grant tokens or change settings.

Once those settings are verified, enable native auto-merge for the ready PR in
GitHub's merge box or with the existing authenticated GitHub CLI:

```bash
gh pr merge <pr-number> --auto --merge --match-head-commit "$(git rev-parse HEAD)"
```

Run this from the PR branch at its pushed head. GitHub retains responsibility
for the required checks and any reviews. A changed head must be tested again; if
`main` advances, update the branch and let CI test the new merge revision. Never
substitute an old run, `--admin`, a direct push, self-approval, or a PAT
embedded in a workflow. If settings or token permissions do not support native
auto-merge, leave the PR open and report the exact prerequisite—do not fall back
to an unprotected merge. This repository intentionally has no privileged PR
checkout/auto-approval workflow.

## Automatic fork publication

A releasing PR updates `fork-release.json` with the next revision before CI: +1
on the same upstream version, or revision 1 when adopting a newer upstream
application version. An unchanged manifest explicitly opts out of publication.
The CI helper job validates this evolution against the current base, so stale
parallel PR version choices must be updated before merging.

After a protected main merge, `Automatic fork release` verifies the exact tested
head/base/tree, creates the immutable manifest tag, and explicitly dispatches
Docker publication on that tag. `GITHUB_TOKEN` tag pushes alone do not trigger
another workflow. No post-merge version commit, PAT or protection bypass is
required. See
[fork maintenance](fork-maintenance.md#automatic-releases-after-a-main-merge)
for idempotence, failed-build recovery, serialization and the numeric `latest`
anti-regression rule. Native app releases remain outside this Docker process.

## Design baseline review

CI visual acceptance uses committed PNG/JSON files under
`e2e/design-system/baselines/{chromium,webkit}/` in the pinned
`mcr.microsoft.com/playwright:v1.58.2-noble` image. Its Playwright version must
match the lockfile; upgrade the image and review any rendering changes in the
same PR. Local `.local/` screenshots are useful comparison evidence but are not
portable CI baselines.

1. Run ordinary PR CI first. A missing or different baseline **fails** the
   acceptance check. It never records the candidate as its own expectation.
2. For the initial bootstrap only, a separate read-only job generates a complete
   `design-baseline-candidate-<SHA>` artifact when no PNG baseline exists. This
   is a separate workspace: its files cannot affect the failing comparison jobs.
   After bootstrap, use **Design baseline candidate** with an exact 40-character
   commit SHA when intentional snapshot changes need evidence. The manual
   workflow generates artifacts only, with no commit/merge rights.
3. An independent reviewer inspects actual pixels and accessibility JSON against
   the intended behavior and upstream/base evidence. A generation run passing is
   **not** approval. Diagnose unrelated failures separately. Record the source
   SHA, pinned image, reviewer, intended differences and review evidence in the
   PR/baseline review record. No per-image user permission is required, but
   authoring and approving the same candidate is not independent review.
4. Commit only reviewed new/changed snapshots to the feature branch; preserve
   unchanged baseline bytes. Never bulk accept unexplained differences or copy
   old local-machine screenshots. The candidate artifact includes exact source
   and environment provenance. Initial bootstrap must cover both browsers;
   missing baseline files remain blocking.
5. Push and let the complete CI run again with snapshot updates disabled. Only
   this new revision's successful **Build Status** can satisfy auto-merge.

The initial review does not require first merging a failing PR: its separate
bootstrap artifact job works before the new manual workflow exists on `main`.
Baseline updates stay normal reviewable source changes, not an automatically
promoted artifact/cache. The initial reference set is roughly 17 MB; monitor
repository growth before adopting a larger collection or external storage.
