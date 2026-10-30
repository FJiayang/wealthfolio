# Reviewed CI design baselines

PNG screenshots and accessibility JSON belong in `chromium/` and `webkit/`. They
are generated in `mcr.microsoft.com/playwright:v1.58.2-noble`, independently
reviewed, and committed. This directory deliberately starts without accepted
snapshots: missing files fail the visual gate until the initial CI artifacts
have been reviewed. Do not substitute the old machine-local `.local/` files.

See
[Design baseline review](../../../docs/development-workflow.md#design-baseline-review)
for bootstrap, provenance, independent review and upgrade rules. Candidate
artifact generation never approves or replaces acceptance baselines.
