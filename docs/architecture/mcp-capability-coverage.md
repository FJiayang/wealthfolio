# MCP business capability coverage

Architecture impact: MCP extends the existing AgentToolCatalog and
AgentEnvironment. Both hosts reuse their existing services, database writer and
domain-event consumers. No global provider settings, new background workers,
migrations, retries or transport lifecycle are added. New business tools belong
only to the MCP catalog; assistant tools keep their existing names and schemas.

## Coverage checklist

Each row maps the frontend adapter (shared entries under
`apps/frontend/src/adapters`, feature entries under `apps/frontend/src`) to its
domain service, MCP tool family, permission and regression coverage. Existing
tools remain supported.

| Frontend adapter                                                     | Service                                                                                                      | MCP tools                                                                                   | Scope                                                            | Regression focus                                             |
| -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- | ------------------------------------------------------------ |
| shared/accounts, shared/portfolios                                   | AccountService, PortfolioService                                                                             | get/list/create/update account; get/create/update portfolio                                 | accounts:read/write                                              | account/portfolio integration, scope denials                 |
| shared/market-data, shared/asset-logos                               | AssetService, AssetLogoService, QuoteService                                                                 | asset profile/create/update, search/resolve, quote mode, logos                              | holdings:read, assets:write, market-data:read/write              | investment creation, disabled providers, quote mode          |
| shared/goals                                                         | GoalService, CurrentAccountValuationService                                                                  | goal CRUD without delete, funding, plan, summary, overviews                                 | financial-planning:read/write; holdings:read for live valuations | goal funding/plan summary and currency                       |
| shared/allocation-targets                                            | AllocationTargetService, DriftService, RebalanceService                                                      | targets, weights, constraints, drift, calculate_rebalance_plan                              | financial-planning:read/write; holdings:read for calculations    | target weights/drift/rebalance; no transactions written      |
| shared/portfolio                                                     | HoldingsService, SnapshotService, ManualSnapshotService                                                      | holdings/lots/valuations/snapshots, prepare/commit holdings                                 | holdings:read/write                                              | preview purity, snapshot overwrite, domain events            |
| shared/activities                                                    | ActivityService                                                                                              | existing activity tools; templates, mapping, transfer pair, duplicate/asset checks          | activities:read/draft/write                                      | existing regression suite; template/transfer integration     |
| adapters/tauri/fire-planner, adapters/web/fire-planner               | existing retirement functions, GoalService                                                                   | retirement projection, Monte Carlo, stress/scenario/sorr/sensitivity                        | financial-planning:read; holdings:read for saved goals           | input validation, existing simulation limits                 |
| shared/exchange-rates, shared/market-data                            | FxService, QuoteService                                                                                      | local quotes, exchange rates, dividends, manual rates; existing manual quote prepare/commit | market-data:read/write                                           | no provider refresh tools; existing quote regression         |
| shared/taxonomies, shared/spending, features/spending/adapters/rules | TaxonomyService, CategorizationRulesService                                                                  | taxonomy/category create/update/move/import, assignments, rules and presets                 | classification:read/suggest/write                                | rule reruns preserve manual edits                            |
| features/spending/adapters                                           | CashActivityService, AnalyticsService, InsightService, BudgetService, EventsService, SpendingSettingsService | spending search/reports/insights/splits/events/budget/settings                              | spending:read/write; classification scopes for categories        | business validation, budget periods, assignment restrictions |
| shared/alternative-assets, shared/contribution-limits                | AlternativeAssetService, loan functions, ContributionLimitService                                            | alternative holdings, loan preview/calculation, deposit calculation                         | holdings:read, financial-planning:read                           | read-only behavior                                           |
| settings/agent-access                                                | PatRepository, shared scope validator                                                                        | permission editing via app management API only                                              | authenticated application management                             | same-session grant/revoke on desktop and web                 |

## Exclusions

No standalone delete/clear/reset tools, arbitrary SQL/file/network access,
credentials, exports, provider refresh/global configuration, health management,
application settings, backups, device/broker connection management, addons, AI
threads or history rebuilds. The existing read-only get_health_status remains
compatible. Alternative asset valuation writes, loan balance actions and
contribution-limit writes are deferred because their runtime orchestration
differs between hosts.

Collection replacement is an update: omitted old entries may be removed. Tools
must state this behavior. Business writes retain existing automatic enrichment,
market sync and recalculation events; excluding standalone refresh tools does
not disable them.

## Asset profile maintenance

`create_asset` and `update_asset_profile` accept the existing asset-owned
`providerConfig` JSON, including the preferred provider and symbol overrides,
under `assets:write`. Global provider settings and credentials remain excluded.

Profile updates follow the existing replacement semantics: before every update,
read `get_asset_profile` and pass the current `name` and complete
`providerConfig` unchanged unless intentionally editing those fields. Omitting
either field or passing `null` clears its stored value. Also retain `notes` and
`displayCode` when changing other fields. `providerConfig` replaces the whole
object; it is not merged. The MCP adapter preserves the asset's current quote
mode; explicit mode changes still use `update_quote_mode` and its existing
permission. Saves retain the existing sync-state reset and domain events. No new
network execution, provider setup or persistence mechanism is introduced.

## Token permission editing

PATCH /api/v1/agent-access/tokens/{id} and mcp_update_token_scopes replace
scopes only. The original credential, fingerprint, expiration and usage metadata
stay unchanged. Both hosts read permissions on each request, including
established MCP sessions. No auth cache or session rebuild is needed. In-flight
requests are not cancelled. Clients may need to refresh tools or reconnect to
discover newly granted tools. Existing scope categories naturally gain the new
capabilities; new categories require explicit grants. No new write permissions
are selected by default.

## MCP-only additions

128 tools are added to the existing 36-tool MCP catalog (164 total). The
assistant remains at 16 read and 5 draft tools. Schemas and scope checks are
owned by the files below in `crates/agent-tools/src/tools/`.

| Implementation                 | Tools                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `account_management.rs`        | `create_account`, `update_account`, `get_account`, `list_accounts`, `create_portfolio`, `update_portfolio_entry`, `get_portfolio`                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `asset_management.rs`          | `create_asset`, `update_asset_profile`, `get_assets`, `get_asset_profile`, `update_quote_mode`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `holdings_management.rs`       | `get_holding`, `get_holdings_list`, `get_asset_holdings`, `get_asset_lots`, `get_snapshots`, `get_snapshot_by_date`, `prepare_holdings_import`, `commit_holdings_import`, `prepare_manual_holdings`, `commit_manual_holdings`, `get_alternative_holdings`, `get_loan_payments`, `calculate_loan`, `recalculate_loan`, `preview_loan_terms`, `calculate_deposits_for_limit`, `get_current_valuation`, `get_historical_valuations`, `get_latest_valuations`                                                                                                                                                  |
| `activity_management.rs`       | `get_activity`, `get_transfer_pair_for_activity`, `find_transfer_match_candidates`, `save_internal_transfer_pair`, `list_import_templates`, `get_import_template`, `get_account_import_mapping`, `save_import_template`, `save_account_import_mapping`, `link_account_template`, `check_existing_duplicates`, `preview_import_assets`                                                                                                                                                                                                                                                                      |
| `goal_management.rs`           | `create_goal`, `update_goal`, `get_goal`, `get_goal_funding`, `get_goal_plan`, `save_goal_funding`, `save_goal_plan`, `refresh_goal_summary`, `get_retirement_overview`, `get_save_up_overview`, `refresh_all_goal_summaries`                                                                                                                                                                                                                                                                                                                                                                              |
| `target_management.rs`         | `create_allocation_target`, `update_allocation_target`, `archive_allocation_target`, `save_allocation_target_weights`, `save_allocation_target_with_weights`, `list_allocation_targets`, `get_allocation_target`, `list_allocation_target_weights`, `list_target_constraints`, `save_target_constraints`, `get_allocation_target_drift`, `calculate_rebalance_plan`                                                                                                                                                                                                                                        |
| `planning_calculations.rs`     | `prepare_retirement_simulation_input`, `calculate_retirement_projection`, `run_retirement_monte_carlo`, `run_retirement_stress_tests`, `run_retirement_scenario_analysis`, `run_retirement_decision_sensitivity_map`, `run_retirement_sorr`, `preview_save_up_overview`                                                                                                                                                                                                                                                                                                                                    |
| `market_queries.rs`            | `list_asset_logos`, `get_asset_logo`, `upsert_asset_logo`, `search_symbol`, `resolve_symbol_quote`, `get_quote_history`, `get_latest_quotes`, `get_exchanges`, `fetch_dividends`, `get_exchange_rates`, `get_exchange_rates_for_dates`, `add_exchange_rate`, `update_exchange_rate`                                                                                                                                                                                                                                                                                                                        |
| `classification_management.rs` | `get_taxonomies`, `get_taxonomy`, `create_taxonomy`, `update_taxonomy`, `create_category`, `update_category`, `move_category`, `import_taxonomy_json`, `assign_asset_to_category`, `replace_asset_taxonomy_assignments`, `list_categorization_rules`, `update_categorization_rule`, `upsert_categorization_rule`, `rerun_categorization_rules`, `list_rule_presets`, `import_rule_preset`                                                                                                                                                                                                                  |
| `spending_management.rs`       | `list_cash_activities`, `search_cash_activities`, `get_activity_assignments`, `get_activity_splits`, `assign_activity_category`, `replace_activity_splits`, `set_activity_event`, `get_spending_settings`, `update_spending_settings`, `get_spending_report`, `get_spending_insight`, `get_event_spending_summaries`, `list_event_types`, `list_events`, `create_event_type`, `update_event_type`, `create_event`, `update_event`, `get_budget`, `upsert_budget_target`, `upsert_budget_rollover_setting`, `create_budget_group`, `update_budget_group`, `assign_category_to_group`, `copy_budget_targets` |

## Regression entry points

- `crates/agent-tools/src/catalog.rs`: every tool is denied before service
  access without scopes; metadata is unique; excluded operations and assistant
  expansion are absent.
- `crates/agent-tools/src/scope.rs`: creation and editing share dependency and
  input validation.
- `crates/storage-sqlite/tests/agent_repos.rs`: scope replacement preserves
  credentials, expiration and concurrent usage metadata.
- `apps/server/tests/agent_access.rs`: same-session permission edits and the
  asset → snapshot → goal/funding/plan → weights → drift → rebalance workflow;
  preview purity, replacement semantics, atomic weights and no generated trades.
- `apps/tauri/src/mcp/tests.rs`: desktop same-session permission changes.
- Frontend agent-access tests, adapter parity and
  `e2e/35-agent-access-permissions.spec.ts`: permission editing and transport,
  keyboard operation and RTL. Test execution results are reported separately;
  the table above identifies regression focus, not a claim that every tool has
  its own end-to-end test.

## Permission categories

The seven new categories are `accounts:write`, `assets:write`, `holdings:write`,
`financial-planning:write`, `market-data:read`, `spending:read` and
`spending:write`. There are now 19 scopes. Existing `market-data:write` still
requires `holdings:read`; activity writes still require activity draft
permission, and classification writes still require classification suggest
permission. Tools that read actual holdings additionally require
`holdings:read`.

## Trade instructions

The MCP catalog exposes `list_trade_instructions`, `get_trade_instruction`,
`create_trade_instruction`, `update_trade_instruction`, `start_trade_instruction`,
`complete_trade_instruction`, `cancel_trade_instruction`,
`correct_trade_instruction_completion_time`, and `delete_trade_instruction`.
Reads require `trade-instructions:read`. All mutations share the single
`trade-instructions:write` permission, whose prerequisites are
`trade-instructions:read`, `accounts:read`, `assets:write`, and
`financial-planning:read`. Existing `trade-instructions:create` grants are
accepted as an alias for the unified write permission; persisted tokens need no
migration. New grants use the canonical write name. Read-only tokens remain
read-only. These tools are MCP-only and redact arguments from audit logs.

UI and MCP use the same service: content edits require PENDING status, all
mutations check the version read from the record, and deletion is permanent for
any status. Updates replace the full editable content; omitted optional fields
are cleared. Completion notes are optional; cancellation notes are required.
Completion records a report, not broker verification or an actual broker order.
MCP completion defaults to server time when omitted; the UI pre-fills current
time and uses server time when cleared. Completion time correction preserves
the original system closure time. Creation retains its request-UUID retry key;
permanent deletion removes that key along with the instruction.

Architecture impact: instructions use their own SQLite table and existing
writer, without device-sync outbox entries, Activity writes, manual quote
writes, or ActivitiesChanged events. Asset identification/creation shares the
manual-entry resolver and preserves its provider policy. An asset created before
an instruction write failure remains registered and can be reused on retry.
Existing activity entry still owns transaction arithmetic, quote-from-activity
writes, and FX setup.

Validation: trade-instruction core/storage/tool tests, existing
activity-resolution regressions, adapter parity tests, and
`e2e/35-trade-instructions.spec.ts` (with `WF_MCP_ENABLED=true`) cover
persistence, permissions, completion correction and UI/MCP operation including
RTL. Instructions are not a multi-user approval or broker order system and are
not replicated to paired devices.

Trade instructions have no top-level navigation or standalone page. Each goal's
detail page contains its own paginated list. The UI supplies the current goal
ID; MCP creation requires `goalId`, and the shared service verifies it before
asset resolution. `notes` is optional in both entry points; the separate
`rationale` field has been removed. The creation sheet reuses the activity type
picker and form sections, showing the form only after Buy or Sell is selected.

Architecture impact: the goal is a required parent. Deleting a goal cascades to
its local instructions; instructions still do not affect goal valuations or
bookkeeping. The pre-release migration rebuilds the experimental instruction
table, intentionally discarding the earlier records without goal associations.
No legacy association UI or migration backfill is provided.
