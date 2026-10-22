# Frontend i18n audit

## Findings and changes

| Source of English text                                 | Treatment                                                                                                                                                                                                                                             |
| ------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Database-seeded taxonomies and hierarchical categories | Display translations keyed by taxonomy/category identity; require the unchanged seed name. Includes instrument types, risk, asset classes, GICS, regions, spending, income, and savings.                                                              |
| Seeded budget groups                                   | Translate unchanged default names by stable group ID. Renamed groups remain verbatim.                                                                                                                                                                 |
| Classification controls and settings                   | Translate headings, selected pills, child menus, trees, profile badges, and search labels. Searches retain original names as keywords. Editable fields remain raw.                                                                                    |
| Holdings and allocation charts                         | Translate built-in labels, synthetic unknown/residual entries, details, and breadcrumbs. Language changes refresh derived chart labels; IDs, amounts, and colors remain based on raw data.                                                            |
| Allocation targets and rebalance                       | Translate preset names/descriptions, risk badges, weight rows, summaries, narratives, and AI tool cards. Replace English noun/plural/target concatenation with translation keys. Existing saved target names stay unchanged.                          |
| Market-data settings                                   | Translate original built-in provider descriptions, functional names, coverage/features, and source-template help. Keep provider brands, custom descriptions, exchange codes, API syntax, and unknown capabilities.                                    |
| Frontend notifications                                 | Translate callback messages for health, updates, portfolio calculations, assets/valuations/liabilities, activities/transfers, spending rules, API keys, custom providers, exports, add-ons, and Agent Access. External error details remain verbatim. |
| Asset editing and import                               | Reuse existing translated instrument/subtype options, translate local schema validation at render time, and translate explanations alongside literal CSV field/type examples.                                                                         |
| Shared display labels                                  | Reuse the existing unavailable label and translate the income tool's year-over-year badge.                                                                                                                                                            |

All 11 supported languages contain the added keys, including their applicable
plural forms. Existing translations are preserved. New translations are drafts;
Chinese financial terms and selected ambiguous instrument labels were reviewed.
Country names are populated from CLDR. Further native-speaker review of other
locales remains useful.

## Intentional exclusions and compatibility

- Brand names, currency/exchange/instrument identifiers, API/JSON/CSS syntax,
  numeric formatting examples, CSV headers/values, and developer test galleries
  remain literal. User-entered names, notes, imported data, provider-defined
  content, and external error messages are not automatically translated.
- Unrecognized IDs and names different from the shipped defaults remain raw.
  Custom groups' categories do not borrow translations from built-in taxonomies.
- A user name identical to its original default is indistinguishable without
  edit history. Its display may be translated; its stored value is preserved.
- No migrations, persistent metadata, dependencies, or backend/API changes are
  introduced. Translation helpers do not alter raw query data or mutation input.

**Architecture impact:** network/provider configuration, background work,
service ownership, events, error propagation, financial calculations, and
user-edit precedence remain unchanged. Localization happens inside frontend
presentation and notification callbacks; no runtime translation service is used.

## Validation

Local checks passed: focused Vitest regressions, all-locale `i18n:check`,
`pnpm type-check`, frontend ESLint (no errors; existing warnings remain), and
`git diff --check`. The hardcoded-string scanner still reports candidates,
including intentional literals; it is not a zero-warning acceptance check.

Focused tests cover unchanged defaults, user renames, same names under different
IDs/taxonomies, synthetic unknown/residual entries, source immutability, live
language changes without extra queries, stable selection IDs, raw form saves,
presets, provider metadata, and Chinese terminology.

The existing PR workflow runs translation parity, sharded frontend unit tests,
frontend builds, and its existing browser harnesses. The new application-level
Playwright spec covers classification renames/persistence and provider metadata
in Chinese and RTL; it belongs to the application E2E suite, which the current
PR workflow does not run. A local attempt stopped before browser execution
because `cargo` was unavailable to the runner. The original `.env.web` was
restored.

Per the requested local-test limit, do not treat this audit as a completed full
suite or a deployment verification. Full checks belong to GitHub Actions after
these changes are committed and pushed; no remote workflow run is implied here.
