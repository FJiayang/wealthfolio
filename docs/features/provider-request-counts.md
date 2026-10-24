# Provider request statistics

The Provider settings page shows application API request attempts for each
built-in provider and custom source (`CUSTOM_SCRAPER:<provider code>`). Testing
a source is included; a source tested before it has a code is shown as
`CUSTOM_SCRAPER:UNSAVED`.

## Counting boundary

One application dispatch counts once, whether it succeeds, fails, or times out.
Explicit retries, pages and yearly history segments each count; a batch sent in
one request counts once. Cache hits, static/local results, validation failures
and disabled providers do not dispatch and do not count. Fallbacks belong to the
provider actually contacted. Automatic HTTP redirects and protocol-level retries
inside the HTTP client are not additional application dispatches.

This is attempt accounting, not a billing reconciliation. The event is recorded
immediately before dispatch, using the UTC timestamp captured before awaiting
the existing database writer. SQLite and the remote API cannot participate in
one atomic transaction: a crash or cancellation between the insert and dispatch
can overcount, while an unavailable database can lose an event. A recording
failure is fail-open and never retries or changes the quote request's result.

## Storage and timezones

Each profile's existing SQLite database stores only an event ID, provider ID and
UTC timestamp. The existing profile write handle inserts events atomically. No
new worker, queue, flush task, request payload storage or synchronization is
added. The table is excluded from device sync; backups of the profile database
retain it.

A new migration is needed because quote sync state describes asset refreshes,
not individual requests, and cannot recover attempts after a restart. UTC event
rows, rather than local-date counters, allow historical statistics to regroup
correctly when the user changes the statistics timezone.

`provider_request_timezone` is independent of portfolio/display timezone. It
defaults to `Asia/Shanghai` (UTC+8), accepts IANA timezones, and is applied only
at query/display time. Queries span 1–366 local calendar days (the UI offers
7/30/90), convert boundaries using timezone/DST rules, and stream matching
events into daily aggregates without retaining request details. The current day
ends at the response's `asOf` snapshot. The migration's existing `run_on`
timestamp supplies `trackingSince`; missing/invalid tracking metadata is an
error, never a fabricated zero. Dates before tracking are unavailable; the first
tracking day and current day are partial.

## Runtime/API

Both Tauri (`get_provider_request_counts`) and web
(`GET /providers/request-counts?days=30`, under the normal API prefix) call the
same core service. The response contains `timezone`, `trackingSince`, `asOf`,
inclusive local `startDate`/`endDate`, and sparse `rows` of `providerId`, `day`,
`requestCount`. The counts query is independent of editing provider settings.
The profile-owned recorder is retained when the quote client is rebuilt after
provider settings change.

Yahoo chart/search sends are local observable equivalents of the SDK's send
layer, while its public response parsers remain in use. The existing chart retry
and response-error behavior are compatibility-tested; upgrades to
`yahoo_finance_api` should review that small compatibility layer.

## Architecture impact

Persistence adds profile-local UTC request events and one independent setting.
Each application send awaits a best-effort insertion through the existing
writer; no network selection, cache policy, fallback order,
certificate/authentication policy, response parsing or explicit retry policy is
intentionally changed. Statistics reads run on a blocking task and propagate
errors only to the separate statistics UI. The API is available in both
runtimes, without device-sync changes.
