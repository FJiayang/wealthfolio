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

## Unreleased

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
