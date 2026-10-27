# Fork release notes

Brief, cumulative release history for
[FJiayang/wealthfolio](https://github.com/FJiayang/wealthfolio). Each entry
lists changes from the preceding fork release, not the full upstream changelog.
Existing features remain supported unless an entry says otherwise.

Tags use `vMAJOR.MINOR.PATCH.REVISION`: the first three components identify the
upstream application version; the last is the fork revision, reset to 1 for a
new upstream version. Application manifests retain the three-component version.
See [fork maintenance](fork-maintenance.md) for provenance and publishing rules.
Historical `v3.10.0.1`–`v3.10.0.7` tags are migrated aliases, not new builds; a
Git tag alone does not establish Docker image availability.

## v3.11.0.11 — 2026-10-27

- Complete missing display translations across supported locales for currency
  selection, asset metadata, performance explanations, AI and synchronization
  screens, settings, validation, notifications, and addon permission
  descriptions. Preserve saved identifiers and user-authored content.
- Integrate the localization branch and latest upstream changes, including theme
  support and reviewed spending workflows. Retain fork trade instructions,
  dual-mode synchronization, diagnostic translations, and the provider request
  diagnostics and query drawer released in v3.11.0.10.
- Adopt upstream MCP categorization-rule and spending-read contracts where names
  overlap, including filtered rule metadata and read-only budget access. Callers
  must use upstream input and output shapes. Retain the fork audit redaction for
  replaced tools.
- Require explicit `spending:read` and `budget:write` grants for the six
  retained fork budget mutation tools, matching upstream's independent budget
  permission. Existing tokens receive no additional grants automatically.
- Retain Docker-only automatic fork tag releases; native jobs remain disabled.

Architecture impact: localization reuses existing i18n boundaries. Integration
preserves fork features while adopting upstream business contracts and scope
separation; internal service adapters reuse the same service instances. No new
permission is silently granted, and audit privacy remains enforced.

## v3.11.0.10 — 2026-10-27

- Record provider-side security codes, explicitly selected business parameters,
  request duration, HTTP status and safe failure categories. Credentials,
  signing keys, authorization headers and raw response bodies are excluded.
- Expose paginated request diagnostics and daily counts through MCP with the
  existing `market-data:read` permission. Keep HTTP attempts and iFinD SDK calls
  separately identified; first-call SDK timing includes initialization.
- Move the request history query into a drawer opened from the market-data page
  toolbar. Clicking a built-in or custom provider's daily count selects that
  provider and today in the configured statistics timezone. Support narrow
  screens and right-to-left layouts.

Architecture impact: extend the existing profile-local SQLite event table and
writer; delete whole events after 30 rolling days, including their historical
counts. Older retained events have unavailable detail and unknown outcomes.
iFinD counts SDK calls rather than session HTTP traffic. Existing provider
routing, credentials, retries, quote persistence and sync boundaries are
preserved; diagnostic storage failures do not change provider results.

## v3.11.0.9 — 2026-10-26

- Add an Actions heading and per-row details, edit and delete menu. Allow
  content edits only while pending, support confirmed permanent deletion in any
  status, and show asset names beside codes using account-style badges.
- Make completion notes optional while keeping cancellation notes required.
  Prefill actual completion with current application-timezone time; clearing it
  uses server time at submission. Preserve unchanged deadlines at full
  precision.
- Expose the full instruction lifecycle through MCP, including editing, status
  changes, completion-time correction and deletion. Use one instruction write
  permission; existing creation grants remain compatible with unified writes,
  while read-only tokens cannot mutate instructions.

Architecture impact: UI and MCP reuse the shared local service and
version-checked writer. Content edits preserve identity and creation retry keys;
permanent deletion removes the record and its key. No schema migration, broker
orders, activity writes or device-sync expansion is introduced. Mutation
arguments remain audit-redacted.

## v3.11.0.8 — 2026-10-26

- Replace trade instruction cards with an activity-style grid, with buy/sell and
  expected completion date filters. Default to earliest expected completion
  first, with missing deadlines last; filtering and sorting run before
  pagination.
- Default to 20 instructions per page, offer 50 and 100, and remember the choice
  in profile-scoped browser preferences. Keep activity-page components
  unchanged.
- Freeze the final column with compact Start, Complete and Cancel actions,
  excluded from column visibility controls. Completion and cancellation retain
  required resolution notes and version checks; balances stay masked when
  copied.

Architecture impact: extend local instruction reads through the existing shared
adapter, Tauri and web paths. Preserve existing polling, status writes, provider
and synchronization boundaries, without a schema migration or new network calls.

## v3.11.0.7 — 2026-10-26

- Add goal-scoped trade instructions with account and asset snapshots, buy/sell
  direction, quantity or amount, optional limit price, notes and expected
  completion time. Instructions do not place broker orders or record activities.
- Let users start, complete or cancel instructions and correct actual completion
  time. Creation supports idempotent retries; status changes check record
  versions to prevent overwriting concurrent edits.
- Expose creation and reads through separately scoped MCP tools; execution
  status changes remain human-only. Reuse themed form controls, localized
  decimal input without losing precision, application timezones and balance
  privacy.

Architecture impact: instructions use a separate profile-local SQLite table and
existing writer, without device-sync outbox entries, Activity writes, manual
quote writes or portfolio recalculation. Manual asset resolution is shared with
activity entry. Deleting a goal also deletes its instructions. This release's
pre-release migration discards experimental instructions without goal links.

## v3.11.0.6 — 2026-10-25

- Replace automatic interval-based market updates with one daily attempt,
  defaulting to 22:00 Asia/Shanghai. Configure hours, minutes and an independent
  timezone in market-data settings. Old interval values remain stored but are no
  longer used.
- Startup and resume check today's schedule; missed runs are caught up only
  later the same day. Failed/interrupted batches need a manual retry that day.
- Activity, account and device-sync changes calculate from saved prices.
  Automatic metadata enrichment runs in the daily batch. Explicit refresh,
  search and data source testing remain available immediately.
- Save selected benchmarks in the securities catalog and update them daily
  without requiring holdings. Performance reads saved history and offers an
  explicit history fetch. Removing a chart comparison keeps the security;
  disable or delete it to stop updates. Unrecognized old selections must be
  selected again.
- Add a local-only recalculation action beside the existing history refresh
  actions.

Architecture impact: network scheduling moves to the shared core coordinator;
profile-local settings store the schedule, last automatic attempt and benchmark
IDs, without a schema migration or device-sync expansion. Existing provider
routing, credential storage, request counting and rate limiting remain in use.
This does not add exchange holiday calendars, system wakeups or daily quota
caps; multiple devices still consume shared provider quotas independently.

## v3.11.0.5 — 2026-10-24

- Add per-provider API request-attempt statistics, including custom sources,
  with daily totals and 7/30/90-day views. Failed requests count; cache hits and
  local lookups do not. Counts are application attempts, not provider billing.
- Include Futu signed HTTP attempts and iFinD MCP HTTP attempts, including
  initialization, stream reconnects and session cleanup, without counting
  individual events on an existing stream.
- Store profile-local UTC events and regroup history using an independent,
  configurable statistics timezone, defaulting to `Asia/Shanghai`. Both desktop
  and web use the same statistics service; events are not device-synced.
- Localize unchanged built-in allocation categories, spending/budget groups and
  event types across settings, forms, reports and assistant tools, plus the
  synthetic portfolio performance label and known holdings-only MWR explanation.
  Preserve custom or renamed labels, stored identities, editing values and
  report totals.

## v3.11.0.4

- Add opt-in [Futu](architecture/futu-market-data-provider.md) signed REST
  quotes and unadjusted daily history for supported US, HK and mainland
  stocks/ETFs; preserve provider settings when updating credentials.
- Add opt-in [Tonghuashun iFinD](architecture/ifind-market-data-provider.md) via
  its MCP service for supported A shares, listed/OTC funds, indices and HK/US
  shares. Supported operations vary by category; HK/US latest prices are the
  reported unadjusted close, not a promise of intraday quotes.

## v3.11.0.3

- Preserve provider failover when a historical segment fails, including invalid
  Eastmoney history status; reject malformed reserved identifiers before search.

## v3.11.0.2

- Replace the external HTTP service integration with built-in Eastmoney, Tencent
  and Sina providers, including exact security/fund lookup and automatic quote
  selection through existing priorities and fallbacks. No Python sidecar is
  needed; coverage varies by provider and operation.
- Migrate HTTP provider bindings/settings offline while preserving assets,
  transactions and stored quotes. See
  [native providers](architecture/native-market-data-providers.md) for migration
  details and limitations; news/statements are provider-layer only.

## v3.11.0.1

- Integrate upstream `v3.11.0` and retain compatible fork features; reset the
  fork revision to 1. Upstream functionality is documented in its
  [release](https://github.com/wealthfolio/wealthfolio/releases/tag/v3.11.0).
- Integrate API-key permission editing and frontend localization fixes, preserve
  user/CSV data, and restore Traditional Chinese sync-interval key parity.

## v3.10.0.8

- Establish four-component fork tags and Docker image versions, validate release
  identity, and document fork/upstream provenance and update limitations.

## v3.10.0.7

- Allow MCP asset creation/profile updates to maintain asset-owned provider
  preferences and symbol overrides. Global provider settings and credentials
  remain excluded; profile configuration updates use replacement semantics.

## v3.10.0.6

- Configure scheduled market-data sync at 1/6/12/24-hour intervals (default: 6);
  startup and manual sync remain unchanged.
- Choose green-up/red-down or red-up/green-down investment gain/loss colors.

## v3.10.0.5

- Expand permission-scoped MCP business tools for accounts/portfolios, assets,
  holdings, activities, spending, classifications, goals and planning; support
  editing existing access-token permissions. See
  [capability coverage](architecture/mcp-capability-coverage.md) for supported
  operations and exclusions.

## v3.10.0.4

- Disable upstream signed native-release workflows for fork tags; this does not
  establish a fork-native updater or signing identity.

## v3.10.0.3

- Localize frontend labels while preserving user-entered names and stored data.

## v3.10.0.2

- Allow explicit GHCR publishing from manual runs targeting a version tag;
  ordinary manual runs remain build/test-only.

## v3.10.0.1

- Add a configurable standard HTTP market-data service and exact asset lookup on
  the then-current upstream 3.10.0 development base. This integration is
  superseded by native providers in `v3.11.0.2`.
- Publish fork Docker images to GHCR only.

## Maintenance

Update this document with each release, using the actual tag-to-tag diff. Keep
pending work marked “Unreleased”; distinguish inherited upstream changes, new
functionality, fixes and migrations. Do not infer deployment availability or
successful validation from a tag or from the presence of tests.
