//! Tool registry with scope-gated execution.

use std::sync::Arc;

use crate::env::AgentEnvironment;
use crate::scope::AgentScopeSet;
use crate::tool::{AgentTool, AgentToolError, AgentToolResult};

/// An ordered collection of agent tools.
///
/// `execute` enforces scopes BEFORE the tool runs — this is the
/// authorization boundary for MCP callers. The in-app assistant filters by
/// its per-thread name allowlist instead and calls tools through the rig
/// adapter, which does not scope-check (the user is the operator there).
pub struct AgentToolCatalog {
    tools: Vec<Arc<dyn AgentTool>>,
}

impl AgentToolCatalog {
    pub fn new(tools: Vec<Arc<dyn AgentTool>>) -> Self {
        Self { tools }
    }

    /// The v1 read-only catalog.
    pub fn v1_read_tools() -> Self {
        Self::new(crate::tools::v1_read_tools())
    }

    /// The in-app assistant catalog: read tools plus the draft/suggest tools.
    /// Commit tools are NOT included — the assistant persists drafts through
    /// its confirmation widget, never directly.
    pub fn assistant_catalog() -> Self {
        let mut tools = crate::tools::v1_read_tools();
        tools.extend(crate::tools::draft_suggest_tools());
        Self::new(tools)
    }

    /// The MCP catalog: read + draft/suggest + commit, import, transfer
    /// linking, quote import and activity update tools. Scope filtering at the
    /// boundary (`execute`, `list_tools`) hides whatever a token can't reach.
    pub fn mcp_catalog() -> Self {
        let mut tools = crate::tools::v1_read_tools();
        tools.extend(crate::tools::draft_suggest_tools());
        tools.extend(crate::tools::commit_tools());
        tools.extend(crate::tools::import_tools());
        tools.extend(crate::tools::transfer_link_tools());
        tools.extend(crate::tools::quote_import_tools());
        tools.extend(crate::tools::activity_update_tools());
        tools.extend(crate::tools::activity_management::tools());
        tools.extend(crate::tools::planning_calculations::tools());
        tools.extend(crate::tools::holdings_management::tools());
        tools.extend(crate::tools::spending_management::tools());
        tools.extend(crate::tools::market_queries::tools());
        tools.extend(crate::tools::classification_management::tools());
        tools.extend(crate::tools::account_management::tools());
        tools.extend(crate::tools::asset_management::tools());
        tools.extend(crate::tools::goal_management::tools());
        tools.extend(crate::tools::target_management::tools());
        tools.extend(crate::tools::trade_instructions::tools());

        Self::new(tools)
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn AgentTool>> {
        self.tools.iter().find(|tool| tool.name() == name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn AgentTool>> {
        self.tools.iter()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Execute `name` with `args`, enforcing `granted` scopes first.
    /// Denials and unknown tools return errors without touching services.
    pub async fn execute(
        &self,
        env: Arc<dyn AgentEnvironment>,
        granted: &AgentScopeSet,
        name: &str,
        args: serde_json::Value,
    ) -> Result<AgentToolResult, AgentToolError> {
        let tool = self
            .get(name)
            .ok_or_else(|| AgentToolError::NotFound(name.to_string()))?;
        let required = tool.required_scopes();
        if !granted.grants_all(required) {
            let missing = required
                .iter()
                .filter(|scope| !granted.contains(**scope))
                .map(|scope| scope.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(AgentToolError::ScopeDenied {
                tool: name.to_string(),
                missing,
            });
        }
        tool.call(env, args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use wealthfolio_spending::categorization_rules::{CategorizationRule, NewCategorizationRule};

    /// Stub environment for authorization tests: service access panics unless
    /// a test explicitly supplies the rule service.
    #[derive(Default)]
    struct PanicEnv {
        rules: Option<Arc<TestRuleService>>,
    }

    #[derive(Default)]
    struct TestRuleService {
        created: Mutex<Vec<NewCategorizationRule>>,
    }

    #[async_trait::async_trait]
    impl wealthfolio_spending::categorization_rules::CategorizationRulesServiceTrait
        for TestRuleService
    {
        async fn update(
            &self,
            _id: &str,
            _patch: wealthfolio_spending::categorization_rules::UpdateCategorizationRule,
        ) -> anyhow::Result<wealthfolio_spending::categorization_rules::CategorizationRule>
        {
            unimplemented!("unused mock operation")
        }

        async fn upsert(
            &self,
            _rule: wealthfolio_spending::categorization_rules::NewCategorizationRule,
        ) -> anyhow::Result<wealthfolio_spending::categorization_rules::CategorizationRule>
        {
            unimplemented!("unused mock operation")
        }

        async fn rerun_all(
            &self,
            _account_ids: &[String],
            _only_uncategorized: bool,
        ) -> anyhow::Result<usize> {
            unimplemented!("unused mock operation")
        }

        async fn list_presets(
            &self,
        ) -> anyhow::Result<Vec<wealthfolio_spending::categorization_rules::RulePresetSummary>>
        {
            unimplemented!("unused mock operation")
        }

        async fn import_preset(
            &self,
            _preset_id: &str,
            _category_resolver: &std::collections::HashMap<String, (String, String)>,
        ) -> anyhow::Result<wealthfolio_spending::categorization_rules::ImportPresetResult>
        {
            unimplemented!("unused mock operation")
        }

        async fn list(&self) -> anyhow::Result<Vec<CategorizationRule>> {
            unreachable!("not used by the commit test")
        }

        async fn create(&self, rule: NewCategorizationRule) -> anyhow::Result<CategorizationRule> {
            let now = chrono::Utc::now().naive_utc();
            let created = CategorizationRule {
                id: rule.id.clone().unwrap(),
                name: rule.name.clone(),
                pattern: rule.pattern.clone(),
                match_type: rule.match_type,
                taxonomy_id: rule.taxonomy_id.clone(),
                category_id: rule.category_id.clone(),
                activity_type: rule.activity_type.clone(),
                amount_op: rule.amount_op,
                amount_value: rule.amount_value,
                amount_value2: rule.amount_value2,
                priority: rule.priority,
                is_global: rule.is_global,
                account_id: rule.account_id.clone(),
                preset_id: rule.preset_id.clone(),
                preset_rule_key: rule.preset_rule_key.clone(),
                preset_version: rule.preset_version.clone(),
                preset_modified: false,
                created_at: now,
                updated_at: now,
            };
            self.created.lock().unwrap().push(rule);
            Ok(created)
        }
    }

    impl AgentEnvironment for PanicEnv {
        fn trade_instruction_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::trade_instructions::TradeInstructionServiceTrait> {
            panic!("unexpected instruction service access")
        }

        fn allocation_target_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolio::allocation_targets::AllocationTargetServiceTrait>
        {
            unimplemented!("unused test service")
        }

        fn drift_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolio::allocation_targets::DriftServiceTrait> {
            unimplemented!("unused test service")
        }

        fn rebalance_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolio::allocation_targets::RebalanceServiceTrait>
        {
            unimplemented!("unused test service")
        }

        fn fx_service(&self) -> Arc<dyn wealthfolio_core::fx::FxServiceTrait> {
            unimplemented!("unused test service")
        }

        fn snapshot_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolio::snapshot::SnapshotServiceTrait> {
            unimplemented!("unused test service")
        }

        fn alternative_asset_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::assets::AlternativeAssetServiceTrait> {
            unimplemented!("unused test service")
        }

        fn asset_logo_service(&self) -> Arc<dyn wealthfolio_core::assets::AssetLogoServiceTrait> {
            unimplemented!("unused test service")
        }

        fn budget_service(&self) -> Arc<wealthfolio_spending::budget::BudgetService> {
            unimplemented!("unused test service")
        }

        fn events_service(&self) -> Arc<wealthfolio_spending::events::EventsService> {
            unimplemented!("unused test service")
        }

        fn spending_settings_service(
            &self,
        ) -> Arc<wealthfolio_spending::settings::SpendingSettingsService> {
            unimplemented!("unused test service")
        }

        fn spending_analytics_service(
            &self,
        ) -> Arc<wealthfolio_spending::analytics::AnalyticsService> {
            unimplemented!("unused test service")
        }

        fn spending_insight_service(&self) -> Arc<wealthfolio_spending::insight::InsightService> {
            unimplemented!("unused test service")
        }

        fn base_currency(&self) -> String {
            "USD".to_string()
        }
        fn account_service(&self) -> Arc<dyn wealthfolio_core::accounts::AccountServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn activity_service(&self) -> Arc<dyn wealthfolio_core::activities::ActivityServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn holdings_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolio::holdings::HoldingsServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn valuation_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolio::valuation::ValuationServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn goal_service(&self) -> Arc<dyn wealthfolio_core::goals::GoalServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn settings_service(&self) -> Arc<dyn wealthfolio_core::settings::SettingsServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn quote_service(&self) -> Arc<dyn wealthfolio_core::quotes::QuoteServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn asset_service(&self) -> Arc<dyn wealthfolio_core::assets::AssetServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn allocation_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolio::allocation::AllocationServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn performance_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolio::performance::PerformanceServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn income_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolio::income::IncomeServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn health_service(&self) -> Arc<dyn wealthfolio_core::health::HealthServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn taxonomy_service(&self) -> Arc<dyn wealthfolio_core::taxonomies::TaxonomyServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn portfolio_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolios::PortfolioServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn net_worth_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::portfolio::net_worth::NetWorthServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn contribution_limit_service(
            &self,
        ) -> Arc<dyn wealthfolio_core::limits::ContributionLimitServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn cash_activity_service(
            &self,
        ) -> Arc<dyn wealthfolio_spending::cash_activities::CashActivityServiceTrait> {
            unimplemented!("PanicEnv")
        }
        fn categorization_rules_service(
            &self,
        ) -> Arc<dyn wealthfolio_spending::categorization_rules::CategorizationRulesServiceTrait>
        {
            self.rules.clone().expect("PanicEnv")
        }
    }

    #[tokio::test]
    async fn every_mcp_tool_denies_before_service_access_without_scopes() {
        let catalog = AgentToolCatalog::mcp_catalog();
        for tool in catalog.iter() {
            let error = catalog
                .execute(
                    Arc::new(PanicEnv::default()),
                    &AgentScopeSet::new(),
                    tool.name(),
                    serde_json::json!({}),
                )
                .await
                .unwrap_err();
            assert!(
                matches!(error, AgentToolError::ScopeDenied { .. }),
                "{}: {error}",
                tool.name()
            );
        }
    }

    #[tokio::test]
    async fn execute_denies_every_v1_tool_with_empty_scopes() {
        let catalog = AgentToolCatalog::v1_read_tools();
        let granted = AgentScopeSet::new();
        for name in catalog.iter().map(|tool| tool.name()).collect::<Vec<_>>() {
            let err = catalog
                .execute(
                    Arc::new(PanicEnv::default()),
                    &granted,
                    name,
                    serde_json::json!({}),
                )
                .await
                .unwrap_err();
            assert!(
                matches!(err, AgentToolError::ScopeDenied { .. }),
                "tool {name} should be scope-denied, got: {err}"
            );
        }
    }

    #[test]
    fn assistant_catalog_excludes_commit_tools() {
        let catalog = AgentToolCatalog::assistant_catalog();
        let names: Vec<&str> = catalog.iter().map(|tool| tool.name()).collect();
        // Draft/suggest tools are present.
        assert!(names.contains(&"record_activity"));
        assert!(names.contains(&"prepare_asset_classification"));
        // Commit and CSV-import tools are NOT exposed to the assistant.
        assert!(!names.contains(&"commit_activity_draft"));
        assert!(!names.contains(&"commit_activity_drafts"));
        assert!(!names.contains(&"commit_asset_classification_draft"));
        assert!(!names.contains(&"commit_categorization_rule"));
        assert!(!names.contains(&"commit_transaction_categories"));
        assert!(!names.contains(&"prepare_activity_import"));
        assert!(!names.contains(&"commit_activity_import"));
        assert!(!names.contains(&"get_import_mapping"));
        assert!(!names.contains(&"find_transfer_matches"));
        assert!(!names.contains(&"link_transfer_activities"));
        assert!(!names.contains(&"unlink_transfer_activities"));
        assert!(!names.contains(&"prepare_quote_import"));
        assert!(!names.contains(&"commit_quote_import"));
        assert!(!names.contains(&"prepare_activity_updates"));
        assert!(!names.contains(&"commit_activity_updates"));
    }

    #[test]
    fn mcp_catalog_includes_commit_and_import_tools() {
        let catalog = AgentToolCatalog::mcp_catalog();
        let names: Vec<&str> = catalog.iter().map(|tool| tool.name()).collect();
        assert!(names.contains(&"commit_activity_draft"));
        assert!(names.contains(&"commit_activity_drafts"));
        assert!(names.contains(&"commit_asset_classification_draft"));
        assert!(names.contains(&"commit_categorization_rule"));
        assert!(names.contains(&"commit_transaction_categories"));
        assert!(names.contains(&"get_import_mapping"));
        assert!(names.contains(&"prepare_activity_import"));
        assert!(names.contains(&"commit_activity_import"));
        assert!(names.contains(&"find_transfer_matches"));
        assert!(names.contains(&"link_transfer_activities"));
        assert!(names.contains(&"unlink_transfer_activities"));
        assert!(names.contains(&"prepare_quote_import"));
        assert!(names.contains(&"commit_quote_import"));
        assert!(names.contains(&"prepare_activity_updates"));
        assert!(names.contains(&"commit_activity_updates"));
        // Read-only token still sees exactly 16 read tools.
        assert_eq!(crate::tools::v1_read_tools().len(), 16);
    }

    #[tokio::test]
    async fn execute_denies_commit_tools_with_empty_scopes() {
        let catalog = AgentToolCatalog::mcp_catalog();
        let granted = AgentScopeSet::new();
        for name in [
            "commit_activity_draft",
            "commit_activity_drafts",
            "commit_asset_classification_draft",
            "commit_categorization_rule",
            "commit_transaction_categories",
            "find_transfer_matches",
            "link_transfer_activities",
            "unlink_transfer_activities",
            "prepare_quote_import",
            "commit_quote_import",
            "prepare_activity_updates",
            "commit_activity_updates",
        ] {
            let err = catalog
                .execute(
                    Arc::new(PanicEnv::default()),
                    &granted,
                    name,
                    serde_json::json!({}),
                )
                .await
                .unwrap_err();
            assert!(
                matches!(err, AgentToolError::ScopeDenied { .. }),
                "tool {name} should be scope-denied, got: {err}"
            );
        }
    }

    #[tokio::test]
    async fn category_commit_requires_both_classification_scopes() {
        let catalog = AgentToolCatalog::mcp_catalog();
        for scopes in [
            vec!["classification:read"],
            vec!["classification:suggest"],
            vec!["classification:write"],
        ] {
            let error = catalog
                .execute(
                    Arc::new(PanicEnv::default()),
                    &AgentScopeSet::from_strs(scopes),
                    "commit_transaction_categories",
                    serde_json::json!({"assignments": []}),
                )
                .await
                .unwrap_err();
            assert!(
                matches!(error, AgentToolError::ScopeDenied { .. }),
                "{error}"
            );
        }
    }

    /// Generated duplicate pairs must be rejected before any service access,
    /// regardless of category, batch position, Unicode or delimiter-containing IDs.
    #[tokio::test]
    async fn category_commit_rejects_generated_duplicate_pairs_before_service_access() {
        let catalog = AgentToolCatalog::mcp_catalog();
        let scopes = AgentScopeSet::from_strs(["classification:suggest", "classification:write"]);
        let mut seed = 0x6a09_e667_f3bc_c909_u64;
        for _ in 0..256 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let count = 2 + (seed as usize % 99);
            let mut assignments: Vec<_> = (0..count)
                .map(|index| {
                    let ch = char::from_u32((seed.wrapping_add(index as u64) % 0x110000) as u32)
                        .unwrap_or('\0');
                    serde_json::json!({
                        "activityId": format!("{index}:{ch}:{seed}"),
                        "taxonomyId": format!("{ch}:taxonomy:{seed}"),
                        "categoryId": format!("category:{index}")
                    })
                })
                .collect();
            let original = seed as usize % count;
            let duplicate = (original + 1 + (seed >> 32) as usize % (count - 1)) % count;
            assignments[duplicate]["activityId"] = assignments[original]["activityId"].clone();
            assignments[duplicate]["taxonomyId"] = assignments[original]["taxonomyId"].clone();
            let error = catalog
                .execute(
                    Arc::new(PanicEnv::default()),
                    &scopes,
                    "commit_transaction_categories",
                    serde_json::json!({"assignments": assignments}),
                )
                .await
                .unwrap_err();
            assert!(
                matches!(error, AgentToolError::InvalidInput(_)),
                "seed {seed}: {error}"
            );
        }
    }

    #[tokio::test]
    async fn categorization_rule_commit_requires_write_scope_and_saves_reviewed_rule() {
        let catalog = AgentToolCatalog::mcp_catalog();
        let rules = Arc::new(TestRuleService::default());
        let env = Arc::new(PanicEnv {
            rules: Some(rules.clone()),
        });
        let input = serde_json::json!({
            "rule": {
                "id": "draft-1", "name": "Coffee", "pattern": "CAFE",
                "matchType": "contains", "taxonomyId": "spending_categories",
                "categoryId": "food", "activityType": null, "amountOp": null,
                "amountValue": null, "amountValue2": null, "priority": 0,
                "isGlobal": true, "accountId": null, "presetId": null,
                "presetRuleKey": null, "presetVersion": null
            }
        });

        let suggest_only = AgentScopeSet::from_strs(["classification:suggest"]);
        let denied = catalog
            .execute(
                env.clone(),
                &suggest_only,
                "commit_categorization_rule",
                input.clone(),
            )
            .await
            .unwrap_err();
        assert!(matches!(denied, AgentToolError::ScopeDenied { .. }));
        assert!(rules.created.lock().unwrap().is_empty());

        let writable = AgentScopeSet::from_strs(["classification:suggest", "classification:write"]);
        let result = catalog
            .execute(env, &writable, "commit_categorization_rule", input)
            .await
            .unwrap();
        assert_eq!(result.content["created"]["id"], "draft-1");
        let saved = rules.created.lock().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].pattern, "CAFE");
    }

    #[tokio::test]
    async fn commit_drafts_rejects_oversized_batch_before_touching_env() {
        // A write-scoped but oversized batch must be rejected by the tool's
        // size cap before any service is reached — PanicEnv proves no write
        // (or any service access) is attempted.
        let catalog = AgentToolCatalog::mcp_catalog();
        let granted = AgentScopeSet::from_strs(["activities:draft", "activities:write"]);
        // Comfortably over the cap (100) regardless of small future tweaks.
        let drafts: Vec<_> = (0..256)
            .map(|_| {
                serde_json::json!({
                    "activityType": "BUY",
                    "activityDate": "2024-01-15",
                    "currency": "USD",
                    "accountId": "a"
                })
            })
            .collect();
        let err = catalog
            .execute(
                Arc::new(PanicEnv::default()),
                &granted,
                "commit_activity_drafts",
                serde_json::json!({ "drafts": drafts }),
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, AgentToolError::InvalidInput(_)),
            "oversized batch should be rejected with InvalidInput, got: {err}"
        );
    }

    #[tokio::test]
    async fn quote_import_needs_holdings_read_and_market_data_write() {
        // Each set lacks one of the two scopes; PanicEnv proves the denial
        // happens before any service is reached.
        let catalog = AgentToolCatalog::mcp_catalog();
        let args = serde_json::json!({ "quotes": [{
            "assetId": "a", "date": "2026-06-30", "price": 10, "currency": "EUR"
        }] });
        for granted in [
            AgentScopeSet::read_activity_write_classification_suggest(),
            AgentScopeSet::from_strs(["market-data:write"]),
        ] {
            for name in ["prepare_quote_import", "commit_quote_import"] {
                let err = catalog
                    .execute(Arc::new(PanicEnv::default()), &granted, name, args.clone())
                    .await
                    .unwrap_err();
                assert!(
                    matches!(err, AgentToolError::ScopeDenied { .. }),
                    "tool {name} should be scope-denied for {granted:?}, got: {err}"
                );
            }
        }
    }

    #[tokio::test]
    async fn quote_import_rejects_oversized_batch_before_touching_env() {
        let catalog = AgentToolCatalog::mcp_catalog();
        let granted = AgentScopeSet::from_strs(["holdings:read", "market-data:write"]);
        let quotes: Vec<_> = (0..256)
            .map(|_| {
                serde_json::json!({
                    "assetId": "a", "date": "2026-06-30", "price": 10, "currency": "EUR"
                })
            })
            .collect();
        for name in ["prepare_quote_import", "commit_quote_import"] {
            let err = catalog
                .execute(
                    Arc::new(PanicEnv::default()),
                    &granted,
                    name,
                    serde_json::json!({ "quotes": quotes }),
                )
                .await
                .unwrap_err();
            assert!(
                matches!(err, AgentToolError::InvalidInput(_)),
                "{name}: oversized batch should be rejected with InvalidInput, got: {err}"
            );
        }
    }

    #[tokio::test]
    async fn activity_update_tools_need_their_scopes() {
        let catalog = AgentToolCatalog::mcp_catalog();
        // Rejected while parsing, so no service is reached (PanicEnv).
        let args = serde_json::json!({ "updates": [{ "activityId": "a", "symbol": "BTC" }] });
        let cases = [
            ("prepare_activity_updates", vec!["activities:read"], false),
            ("prepare_activity_updates", vec!["activities:draft"], false),
            (
                "prepare_activity_updates",
                vec!["activities:read", "activities:draft"],
                true,
            ),
            (
                "commit_activity_updates",
                vec!["activities:read", "activities:draft"],
                false,
            ),
            (
                "commit_activity_updates",
                vec!["activities:draft", "activities:write"],
                false,
            ),
            (
                "commit_activity_updates",
                vec!["activities:read", "activities:draft", "activities:write"],
                true,
            ),
        ];
        for (name, scopes, allowed) in cases {
            let result = catalog
                .execute(
                    Arc::new(PanicEnv::default()),
                    &AgentScopeSet::from_strs(scopes.clone()),
                    name,
                    args.clone(),
                )
                .await;
            assert_eq!(
                !matches!(result, Err(AgentToolError::ScopeDenied { .. })),
                allowed,
                "{name} with {scopes:?}"
            );
        }
    }

    #[tokio::test]
    async fn activity_updates_refuse_symbols_and_bad_batches_before_touching_env() {
        let catalog = AgentToolCatalog::mcp_catalog();
        let granted =
            AgentScopeSet::from_strs(["activities:read", "activities:draft", "activities:write"]);
        let rows = serde_json::json!({ "updates": [
            { "activityId": "a", "symbol": "BTC" },
            { "activityId": "b", "assetId": "x", "ticker": "BTC" },
            { "activityId": "c", "unitprice": 1 }
        ]});
        let preview = catalog
            .execute(
                Arc::new(PanicEnv::default()),
                &granted,
                "prepare_activity_updates",
                rows.clone(),
            )
            .await
            .unwrap();
        assert_eq!(
            preview.content["summary"]["invalid"], 3,
            "{}",
            preview.content
        );
        let committed = catalog
            .execute(
                Arc::new(PanicEnv::default()),
                &granted,
                "commit_activity_updates",
                rows,
            )
            .await
            .unwrap();
        assert_eq!(committed.content["updated"], serde_json::json!([]));
        assert_eq!(committed.content["errors"].as_array().unwrap().len(), 3);

        let oversized: Vec<_> = (0..101)
            .map(|index| serde_json::json!({ "activityId": format!("a{index}"), "fee": 1 }))
            .collect();
        let err = catalog
            .execute(
                Arc::new(PanicEnv::default()),
                &granted,
                "commit_activity_updates",
                serde_json::json!({ "updates": oversized }),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AgentToolError::InvalidInput(_)), "{err}");
    }

    #[tokio::test]
    async fn execute_returns_not_found_for_unknown_tool() {
        let catalog = AgentToolCatalog::v1_read_tools();
        let err = catalog
            .execute(
                Arc::new(PanicEnv::default()),
                &AgentScopeSet::read_only(),
                "no_such_tool",
                serde_json::json!({}),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AgentToolError::NotFound(_)));
    }
}

#[cfg(test)]
mod business_catalog_tests {
    use super::*;

    #[test]
    fn business_tools_are_mcp_only_and_exclude_system_controls() {
        let mcp = AgentToolCatalog::mcp_catalog();
        let assistant = AgentToolCatalog::assistant_catalog();
        let mut names = std::collections::HashSet::new();
        for tool in mcp.iter() {
            assert!(names.insert(tool.name()), "duplicate tool {}", tool.name());
            assert!(
                !tool.required_scopes().is_empty(),
                "unprotected tool {}",
                tool.name()
            );
            assert_eq!(tool.input_schema()["type"], "object", "{}", tool.name());
        }
        for required in [
            "create_asset",
            "create_goal",
            "save_goal_plan",
            "save_allocation_target_with_weights",
            "get_allocation_target_drift",
            "calculate_rebalance_plan",
            "prepare_holdings_import",
            "commit_holdings_import",
            "get_spending_report",
            "copy_budget_targets",
        ] {
            assert!(mcp.get(required).is_some(), "missing {required}");
            assert!(
                assistant.get(required).is_none(),
                "assistant gained {required}"
            );
        }
        for forbidden in [
            "delete_account",
            "delete_asset",
            "delete_activity",
            "delete_goal",
            "delete_snapshot",
            "sync_market_data",
            "synch_quotes",
            "update_settings",
            "execute_health_fix",
            "recalculate_portfolio",
            "update_portfolio",
            "reset_budget_groups",
            "export_data",
            "mcp_update_token_scopes",
        ] {
            assert!(mcp.get(forbidden).is_none(), "forbidden tool {forbidden}");
        }
    }
    #[test]
    fn business_audit_omits_financial_inputs_and_error_details() {
        let catalog = AgentToolCatalog::mcp_catalog();
        let business_tools = [
            crate::tools::account_management::tools(),
            crate::tools::asset_management::tools(),
            crate::tools::holdings_management::tools(),
            crate::tools::activity_management::tools(),
            crate::tools::goal_management::tools(),
            crate::tools::target_management::tools(),
            crate::tools::planning_calculations::tools(),
            crate::tools::market_queries::tools(),
            crate::tools::classification_management::tools(),
            crate::tools::spending_management::tools(),
        ];
        for tool in business_tools.into_iter().flatten() {
            let tool = catalog.get(tool.name()).unwrap();
            let private = "private financial input";
            assert_eq!(
                tool.sanitize_args_for_audit(&serde_json::json!({"input":private})),
                serde_json::Value::Null
            );
            assert!(!tool
                .sanitize_error_for_audit(&AgentToolError::ExecutionFailed(private.into()))
                .contains(private));
        }
    }

    #[test]
    fn business_schemas_match_domain_enums_and_search_pagination() {
        let catalog = AgentToolCatalog::mcp_catalog();
        let rule = catalog
            .get("upsert_categorization_rule")
            .unwrap()
            .input_schema();
        let actual =
            serde_json::to_value(wealthfolio_spending::categorization_rules::RuleMatchType::Exact)
                .unwrap();
        assert!(
            rule["properties"]["input"]["properties"]["matchType"]["enum"]
                .as_array()
                .unwrap()
                .contains(&actual)
        );
        let budget = catalog.get("upsert_budget_target").unwrap().input_schema();
        let actual =
            serde_json::to_value(wealthfolio_spending::budget::BudgetTargetType::GroupBuffer)
                .unwrap();
        assert!(
            budget["properties"]["input"]["properties"]["targetType"]["enum"]
                .as_array()
                .unwrap()
                .contains(&actual)
        );
        let search = catalog
            .get("search_cash_activities")
            .unwrap()
            .input_schema();
        for field in ["offset", "limit", "sortDir", "includeCashBalances"] {
            assert!(
                search["properties"]["request"]["properties"]
                    .get(field)
                    .is_some(),
                "missing {field}"
            );
        }
    }
}
