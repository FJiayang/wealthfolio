# Reviewed CI design baselines

PNG screenshots and accessibility JSON belong in `chromium/` and `webkit/`.
Current candidates use `mcr.microsoft.com/playwright:v1.65.0-noble`; existing
baselines and their review provenance are recorded in `REVIEW.md`. Changes must
be independently reviewed before they are committed. Missing files fail the
visual gate. Do not substitute the old machine-local `.local/` files.

See
[Design baseline review](../../../docs/development-workflow.md#design-baseline-review)
for bootstrap, provenance, independent review and upgrade rules. Candidate
artifact generation never approves or replaces acceptance baselines.
