# AGENTS.md

Shared instructions for agents working on Wealthfolio, a local-first finance app
with a React frontend, Tauri desktop/mobile runtime, and Axum web server.

## Working agreements

- Resolve routine implementation choices from nearby code. State material
  assumptions; ask when ambiguity changes scope, public behavior, data
  compatibility, or an irreversible action.
- Prefer the smallest readable change that satisfies the task. Avoid speculative
  features and abstractions; preserve validation at external input and
  persistence boundaries.
- Reuse before adding infrastructure: trace the existing service, event, queue,
  and error handling before proposing a new mechanism. Extend those first.
- Before adding a migration, persisted state, worker, retry mechanism, or
  synchronization primitive, explain the concrete failure it prevents and why
  existing mechanisms cannot handle it. Separate required behavior from optional
  resilience; do not add optional resilience without agreement.
- Match existing style. Do not refactor unrelated code or reformat adjacent
  files. Remove only imports, variables, and functions made unused by your
  changes.
- Preserve existing user changes. Report unrelated issues rather than fixing
  them.
- For multi-step work, give a brief plan with verification steps. Keep plans
  clear and concise; identify what is reused, added, and deferred, and list
  unresolved questions only when they need an answer.

## Architecture boundaries for fixes and refactors

Before implementation, trace the affected execution path and identify whether
proposed changes alter network calls or provider settings, synchronous versus
background execution, persistence or events, ownership of business logic,
failure propagation, retries, or user-edit precedence.

For small fixes and behavior-preserving refactors, preserve these boundaries. If
a boundary change is necessary, explain the previous and proposed behavior, why
the existing mechanism is insufficient, and the required validation before
proceeding. Include an explicit **Architecture impact** note in the plan and PR
description: either name the boundaries that remain unchanged, or describe the
changes and their validation. Revisit this note if the implementation expands.
Do not call a change a refactor if it changes observable behavior, side effects,
or execution timing.

Test architectural invariants as well as successful outcomes: disabled providers
receive no requests, local resolution does not acquire new network dependencies,
provider failures stay isolated, and enrichment preserves user edits. In final
reports and PR descriptions, state changed boundaries and distinguish tested
guarantees from unverified assumptions.

## Architecture and implementation

- Frontend code lives in `apps/frontend/src/`; shared TypeScript packages live
  in `packages/`. Reuse `@wealthfolio/ui` components and existing feature
  patterns.
- Frontend calls go through `@/adapters`. `apps/frontend/vite.config.ts` selects
  Tauri or web adapters at build time using `BUILD_TARGET`; `adapters/index.ts`
  defaults to Tauri for TypeScript checking, as does the `#platform` alias in
  `apps/frontend/tsconfig.json`. TypeScript includes the frontend source tree,
  but these aliases do not validate both runtime configurations equivalently.
- Shared domain calls live in `apps/frontend/src/adapters/shared/` and use
  `shared/platform.ts`, which resolves through `#platform`. Runtime-specific
  operations live in `adapters/tauri/` and `adapters/web/`; some features own
  local adapters. Follow the relevant existing domain instead of creating a new
  layer.
- Keep Tauri commands (`apps/tauri/src/commands/`) and Axum handlers
  (`apps/server/src/api/`) thin. Put shared business logic in the owning Rust
  crate; core services live in `crates/core/`, persistence and migrations in
  `crates/storage-sqlite/`. Add schema changes as new migrations in
  `crates/storage-sqlite/migrations/`; never edit a migration already shipped.
- Portfolio reads in `crates/core` that load a whole history or run the engine
  (performance, valuation history) run off the async workers inside their
  service, through `blocking` in `crates/core/src/portfolio/coordinator/mod.rs`,
  so callers await them directly rather than wrapping them in `spawn_blocking`.
  Give new history-sized reads the same treatment.
- Classifications of core records, such as which asset kinds are alternative,
  belong to `crates/core`. The portfolio engine receives them as facts that
  `crates/core/src/portfolio/coordinator/facts.rs` derives, as it does
  `Asset::contract_multiplier`; the engine must not keep its own copy of such a
  rule. Core models may call the engine only to ask about the engine's own
  capabilities, such as which cost basis methods it computes.

When adding or changing a backend call, trace both runtime paths: the frontend
adapter/export, Tauri command registration in `apps/tauri/src/lib.rs`, web
command mapping in `apps/frontend/src/adapters/web/core.ts`, and Axum
route/handler. Use `apps/frontend/src/adapters/shared/accounts.ts` as a
shared-call example and
`apps/frontend/src/adapters/adapter-command-parity.test.ts` to check wiring.

For UI work, use React Router in `apps/frontend/src/routes.tsx`, existing
react-hook-form/Zod form patterns, and theme tokens in
`apps/frontend/src/globals.css`. Prefer interfaces for object shapes, named
component exports, and lowercase-with-dashes directories; avoid TypeScript
enums. For Rust domain errors, follow the existing `Result`/`Option` and
`thiserror` patterns. Clippy rejects `unwrap()` and `panic!` outside tests:
return an error with `?`, or use `expect("why it cannot fail")` for a proven
invariant.

UI must work right-to-left: logical classes and inline styles (`ms-`, `start-`,
`text-end`, `insetInlineStart`), physical only where the side is literal;
`rtl:rotate-180` on forward/back icons; `inline-start`/`inline-end` popup sides;
`useDirection()` for key or scroll handling; values via the formatting hooks.
Details in `apps/frontend/src/i18n/README.md`.

## Persisted identifiers and security

- Reuse named constants for secret-store keys and identifiers shared across code
  paths. Put shared constants in the owning module/crate; preserve persisted key
  values when replacing literals.
- Preserve local-first SQLite storage and existing opt-in broker/device-sync
  boundaries. Do not introduce new storage or transmission of financial data
  outside the requested scope.
- Access secrets through `SecretStore` in `crates/core/src/secrets/mod.rs`.
  Tauri uses native credential storage (`apps/tauri/src/secret_store.rs`); the
  web server uses a file-backed store with configured encryption
  (`apps/server/src/secrets/mod.rs`, `apps/server/src/config.rs`). Preserve
  these protections; do not add plaintext persistence or browser localStorage
  for secrets.
- Never log secrets or financial data.

## Setup and commands

Run commands from the repository root unless noted. Use the Node version in
`.node-version`, pnpm version in `package.json`, and Rust toolchain in
`rust-toolchain.toml`. Install JS dependencies with
`pnpm install --frozen-lockfile`. Build package declarations with
`pnpm build:types` before standalone frontend checks/builds in a fresh checkout;
`pnpm type-check` includes that step.

| Task                       | Command / scope                                                                |
| -------------------------- | ------------------------------------------------------------------------------ |
| Desktop development        | `pnpm tauri dev` (persistent dev process)                                      |
| Web development            | `pnpm dev:web` (persistent dev process)                                        |
| TS tests, one run          | `pnpm --filter frontend exec vitest run` (append a test path for focused runs) |
| TS formatting, lint, types | `pnpm check` (does not run tests, builds, or Rust checks)                      |
| TS type checks             | `pnpm type-check`                                                              |
| Web frontend build         | `pnpm build`                                                                   |
| Tauri frontend build       | `pnpm build:tauri` (does not compile Rust)                                     |
| Focused Rust tests         | `cargo test --locked -p <crate> <test_filter>`                                 |
| Rust runtime compilation   | `cargo check --locked -p wealthfolio-app -p wealthfolio-server`                |

## Development and validation workflow

1. Fetch the latest fork `main` and start a feature branch from it:
   `git fetch origin main && git switch -c feature/<name> origin/main`. Do not
   develop directly on `main` or start from a stale local branch.
2. Implement the change and run the relevant **one-shot local unit/regression
   tests**. Add focused lint/type/static checks when appropriate. Record their
   commands and results; do not run the entire test suite or every browser suite
   locally for each change.
3. After focused tests pass, commit, push the feature branch, and create a PR
   against `main`. Explain the architecture impact and any unverified behavior.
4. CI owns the full Rust/TypeScript/script/locale checks, both frontend builds,
   and complete deterministic browser suites. Wait for `PR Check`'s **Build
   Status** on the current revision. Failed, canceled, or unexpectedly skipped
   required jobs block merging; never bypass them or retry until green without
   diagnosing the failure.
5. Enable GitHub's native auto-merge for the ready PR only after confirming that
   `main` requires **Build Status** from GitHub Actions with strict up-to-date
   protection. GitHub merges after those checks and any configured reviews pass.
   Do not force merge, self-approve, embed a PAT, or weaken protections. A
   changed head/base needs fresh CI; do not treat an earlier green run as proof.

A releasing PR also updates `fork-release.json`: increment the revision by one
on the same upstream version, or reset to 1 for a newer integrated upstream
application version. Unchanged metadata means no release. The required CI helper
checks this against the current base; after merging, automatic publication tags
the exact tested merge and dispatches Docker without a PAT or a main write.
Follow `docs/fork-maintenance.md` for recovery; never move an existing tag.

For focused local verification:

- **Documentation/CI:** check referenced commands, workflow/helper tests,
  formatting, and the diff; no application build or browser run is necessary.
- **Frontend/shared TS:** run the relevant Vitest files with
  `pnpm --filter frontend exec vitest run <test-path>` and appropriate focused
  static checks. CI runs all tests, types, lint, and web/Tauri frontend builds.
- **Adapter/API wiring:** include the adapter-command-parity test and affected
  handler/service tests. CI compiles the shared runtime consumers.
- **Rust/persistence:** run `cargo test --locked -p <crate> <test_filter>`,
  formatting and focused Clippy as needed. CI runs workspace checks/tests and
  the cross-platform HTTP and secret-store compatibility suites.
- **UI text/translations:** update affected keys/plurals in every supported
  locale and run `pnpm --filter frontend i18n:check`.
- **UI behavior/layout:** add or update the regression test, including RTL. A
  focused browser reproduction is useful when needed, not a requirement to run
  all browsers locally. Consult `.claude/skills/run-e2e-tests/SKILL.md` and
  `e2e/README.md` before any local E2E run; use disposable test data.

CI browser suites must not suppress assertions, hide skips, increase retries to
obtain a pass, or regenerate screenshots as their own expected result. Missing
or changed design snapshots fail; candidate artifacts require independent review
before committing accepted baseline changes. Follow
[the development workflow](docs/development-workflow.md) for baseline bootstrap,
merge protection, and failure diagnosis.

Report checks that failed, were blocked, or were not run and why. Focused local
checks are not a full-suite pass. Review the final diff for unrelated changes.

## Code review rules

Challenge whether each new mechanism needs to exist, not only whether it works.

For bug-fix reviews:

- Trace the reported symptom through the actual execution path before judging
  the fix. Establish why the original code fails and how the change prevents it.
- Seek a reproduction or regression test that fails before and passes after.
- Distinguish confirmed findings, hypotheses, and unverified behavior.
- “No regressions found” does not mean “the reported bug is fixed.” If
  root-cause evidence is missing, explicitly conclude **“fix not verified”**; do
  not imply approval.
- Inspect the exact PR revision and report validation limitations.
