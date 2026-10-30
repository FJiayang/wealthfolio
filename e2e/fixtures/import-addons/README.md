# Import menu host fixtures

`fixture.ts` generates two deliberately tiny, import-only addons at test
runtime. Their readable manifest and JavaScript are stored in an uncompressed
ZIP made with Node 24's built-in CRC32 and Buffer APIs; no new library or
packaging tool is needed. The regular host install endpoint, manifest
validation, sandbox, permission bridge, menu contributions and import wizard are
exercised rather than mocked.

Each addon declares one route and one investment/spending slot, no sidebar
entry, and only the `activities.openImport` function. Its runtime registration
uses the nonempty, addon-owned `/addons/<id>` path required by the host, and
`enable` awaits the real route-registration acknowledgement. A DOM button hands
one synthetic USD `DEPOSIT`/`WITHDRAWAL` draft to the host; the latter is the
canonical spending cash outflow, not a separate `EXPENSE` activity type. It does
not parse files, request network access, or save activities. Test-specific UUIDs
and account IDs prevent a prior installation from satisfying assertions. The
test cancels the wizard and cleans up only its own addons/account, restoring the
settings it changed.

This fixture tests the host contract. Third-party PDF/CSV parsing, matching and
preview-grid presentation are intentionally not part of this repository's E2E.

The mobile cases use a real phone user agent/touch context at the tested narrow
viewport and cancel through the header Back button. Desktop cases use the Return
to original transactions button. Both paths must show the same cancel
confirmation and return to the originating addon without saving a draft.
