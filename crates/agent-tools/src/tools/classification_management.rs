//! MCP-only business tools. Domain services own validation and persistence.
use super::business_support::*;
use crate::{
    AgentEnvironment, AgentScope, AgentTool, AgentToolAccess, AgentToolError, AgentToolResult,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetTaxonomiesArgs {}

business_tool!(
    GetTaxonomies,
    GetTaxonomiesArgs,
    "get_taxonomies",
    "Read complete taxonomies and categories for asset or activity classification.",
    json!({"type":"object","properties":{},"required":[]}),
    [AgentScope::ClassificationRead],
    Read,
    |env, args| {
        let _ = args;
        output(env.taxonomy_service().get_taxonomies_with_categories()?)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetTaxonomyArgs {
    pub id: String,
}

business_tool!(
    GetTaxonomy,
    GetTaxonomyArgs,
    "get_taxonomy",
    "Read complete taxonomies and categories for asset or activity classification.",
    json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}),
    [AgentScope::ClassificationRead],
    Read,
    |env, args| { output(env.taxonomy_service().get_taxonomy(&args.id)?) }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateTaxonomyArgs {
    pub input: wealthfolio_core::taxonomies::NewTaxonomy,
}

business_tool!(CreateTaxonomy, CreateTaxonomyArgs, "create_taxonomy",
    "Save taxonomy/category configuration through existing domain validation. No deletion or system migration is performed.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"id":{"type":"string"},"name":{"type":"string"},"color":{"type":"string"},"description":{"type":"string"},"isSystem":{"type":"boolean"},"isSingleSelect":{"type":"boolean"},"sortOrder":{"type":"integer"},"scope":{"type":"string","enum":["asset","activity"]},"createdAt":{"type":"string"},"updatedAt":{"type":"string"}},"required":["name","color","isSystem","isSingleSelect","sortOrder"]}},"required":["input"]}), [AgentScope::ClassificationSuggest, AgentScope::ClassificationWrite], Write, |env, args| {
        output(env.taxonomy_service().create_taxonomy(args.input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateTaxonomyArgs {
    pub input: wealthfolio_core::taxonomies::Taxonomy,
}

business_tool!(UpdateTaxonomy, UpdateTaxonomyArgs, "update_taxonomy",
    "Save taxonomy/category configuration through existing domain validation. No deletion or system migration is performed.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"id":{"type":"string"},"name":{"type":"string"},"color":{"type":"string"},"description":{"type":"string"},"isSystem":{"type":"boolean"},"isSingleSelect":{"type":"boolean"},"sortOrder":{"type":"integer"},"scope":{"type":"string","enum":["asset","activity"]},"createdAt":{"type":"string"},"updatedAt":{"type":"string"}},"required":["name","color","isSystem","isSingleSelect","sortOrder","id","createdAt","updatedAt"]}},"required":["input"]}), [AgentScope::ClassificationSuggest, AgentScope::ClassificationWrite], Write, |env, args| {
        output(env.taxonomy_service().update_taxonomy(args.input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateCategoryArgs {
    pub input: wealthfolio_core::taxonomies::NewCategory,
}

business_tool!(CreateCategory, CreateCategoryArgs, "create_category",
    "Save taxonomy/category configuration through existing domain validation. No deletion or system migration is performed.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"id":{"type":"string"},"taxonomyId":{"type":"string"},"parentId":{"type":"string"},"name":{"type":"string"},"key":{"type":"string"},"color":{"type":"string"},"description":{"type":"string"},"sortOrder":{"type":"integer"},"icon":{"type":"string"},"createdAt":{"type":"string"},"updatedAt":{"type":"string"}},"required":["taxonomyId","name","key","color","sortOrder"]}},"required":["input"]}), [AgentScope::ClassificationSuggest, AgentScope::ClassificationWrite], Write, |env, args| {
        output(env.taxonomy_service().create_category(args.input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateCategoryArgs {
    pub input: wealthfolio_core::taxonomies::Category,
}

business_tool!(UpdateCategory, UpdateCategoryArgs, "update_category",
    "Save taxonomy/category configuration through existing domain validation. No deletion or system migration is performed.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"id":{"type":"string"},"taxonomyId":{"type":"string"},"parentId":{"type":"string"},"name":{"type":"string"},"key":{"type":"string"},"color":{"type":"string"},"description":{"type":"string"},"sortOrder":{"type":"integer"},"icon":{"type":"string"},"createdAt":{"type":"string"},"updatedAt":{"type":"string"}},"required":["taxonomyId","name","key","color","sortOrder","id","createdAt","updatedAt"]}},"required":["input"]}), [AgentScope::ClassificationSuggest, AgentScope::ClassificationWrite], Write, |env, args| {
        output(env.taxonomy_service().update_category(args.input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MoveCategoryArgs {
    pub taxonomy_id: String,
    pub category_id: String,
    #[serde(default)]
    pub new_parent_id: Option<String>,
    pub position: i32,
}

business_tool!(
    MoveCategory,
    MoveCategoryArgs,
    "move_category",
    "Move a category within its taxonomy using existing hierarchy validation.",
    json!({"type":"object","properties":{"taxonomyId":{"type":"string"},"categoryId":{"type":"string"},"newParentId":{"type":"string"},"position":{"type":"integer"}},"required":["taxonomyId","categoryId","position"]}),
    [
        AgentScope::ClassificationSuggest,
        AgentScope::ClassificationWrite
    ],
    Write,
    |env, args| {
        output(
            env.taxonomy_service()
                .move_category(
                    &args.taxonomy_id,
                    &args.category_id,
                    args.new_parent_id,
                    args.position,
                )
                .await?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportTaxonomyJsonArgs {
    pub content: String,
}

business_tool!(ImportTaxonomyJson, ImportTaxonomyJsonArgs, "import_taxonomy_json",
    "Import taxonomy JSON content through the existing validator. Does not read files or export data.",
    json!({"type":"object","properties":{"content":{"type":"string"}},"required":["content"]}), [AgentScope::ClassificationSuggest, AgentScope::ClassificationWrite], Write, |env, args| {
        output(env.taxonomy_service().import_taxonomy_json(&args.content).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AssignAssetToCategoryArgs {
    pub input: wealthfolio_core::taxonomies::NewAssetTaxonomyAssignment,
}

business_tool!(
    AssignAssetToCategory,
    AssignAssetToCategoryArgs,
    "assign_asset_to_category",
    "Assign an asset to a taxonomy category with a basis-point weight.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"assetId":{"type":"string"},"taxonomyId":{"type":"string"},"categoryId":{"type":"string"},"weight":{"type":"integer"},"source":{"type":"string"}},"required":["assetId","taxonomyId","categoryId","weight","source"]}},"required":["input"]}),
    [
        AgentScope::ClassificationSuggest,
        AgentScope::ClassificationWrite
    ],
    Write,
    |env, args| {
        output(
            env.taxonomy_service()
                .assign_asset_to_category(args.input)
                .await?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReplaceAssetTaxonomyAssignmentsArgs {
    pub asset_id: String,
    pub taxonomy_id: String,
    pub assignments: Vec<wealthfolio_core::taxonomies::NewAssetTaxonomyAssignment>,
}

business_tool!(ReplaceAssetTaxonomyAssignments, ReplaceAssetTaxonomyAssignmentsArgs, "replace_asset_taxonomy_assignments",
    "Replace the complete category assignment list for one asset and taxonomy. Omitted assignments are removed; the service validates IDs and weights.",
    json!({"type":"object","properties":{"assetId":{"type":"string"},"taxonomyId":{"type":"string"},"assignments":{"type":"array","items":{"type":"object","properties":{"assetId":{"type":"string"},"taxonomyId":{"type":"string"},"categoryId":{"type":"string"},"weight":{"type":"integer"},"source":{"type":"string"}},"required":["assetId","taxonomyId","categoryId","weight","source"]},"maxItems":100}},"required":["assetId","taxonomyId","assignments"]}), [AgentScope::ClassificationSuggest, AgentScope::ClassificationWrite], Write, |env, args| {
        if args.assignments.len() > 100 { return Err(AgentToolError::InvalidInput("At most 100 items per call".into())); }
        output(env.taxonomy_service().replace_asset_taxonomy_assignments(&args.asset_id, &args.taxonomy_id, args.assignments).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateCategorizationRuleArgs {
    pub id: String,
    pub patch: wealthfolio_spending::categorization_rules::UpdateCategorizationRule,
}

business_tool!(
    UpdateCategorizationRule,
    UpdateCategorizationRuleArgs,
    "update_categorization_rule",
    "Update a categorization rule with existing pattern, amount and account-scope validation.",
    json!({"type":"object","properties":{"id":{"type":"string"},"patch":{"type":"object","properties":{"id":{"type":"string"},"name":{"type":"string"},"pattern":{"type":"string"},"matchType":{"type":"string","enum":["contains","starts_with","exact","regex"]},"taxonomyId":{"type":"string"},"categoryId":{"type":"string"},"activityType":{"type":"string"},"amountOp":{"type":"string","enum":["eq","gt","gte","lt","lte","between"]},"amountValue":{"oneOf":[{"type":"string"},{"type":"number"}]},"amountValue2":{"oneOf":[{"type":"string"},{"type":"number"}]},"priority":{"type":"integer"},"isGlobal":{"type":"boolean"},"accountId":{"type":"string"}},"required":[]}},"required":["id","patch"]}),
    [
        AgentScope::ClassificationSuggest,
        AgentScope::ClassificationWrite
    ],
    Write,
    |env, args| {
        let saved = env
            .categorization_rules_service()
            .update(&args.id, args.patch)
            .await?;
        auto_categorize_opted_in(env.clone()).await;
        output(saved)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpsertCategorizationRuleArgs {
    pub input: wealthfolio_spending::categorization_rules::NewCategorizationRule,
}

business_tool!(
    UpsertCategorizationRule,
    UpsertCategorizationRuleArgs,
    "upsert_categorization_rule",
    "Save a complete categorization rule by ID atomically, using existing validation.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"id":{"type":"string"},"name":{"type":"string"},"pattern":{"type":"string"},"matchType":{"type":"string","enum":["contains","starts_with","exact","regex"]},"taxonomyId":{"type":"string"},"categoryId":{"type":"string"},"activityType":{"type":"string"},"amountOp":{"type":"string","enum":["eq","gt","gte","lt","lte","between"]},"amountValue":{"oneOf":[{"type":"string"},{"type":"number"}]},"amountValue2":{"oneOf":[{"type":"string"},{"type":"number"}]},"priority":{"type":"integer"},"isGlobal":{"type":"boolean"},"accountId":{"type":"string"}},"required":["id","name","pattern"]}},"required":["input"]}),
    [
        AgentScope::ClassificationSuggest,
        AgentScope::ClassificationWrite
    ],
    Write,
    |env, args| {
        let saved = env
            .categorization_rules_service()
            .upsert(args.input)
            .await?;
        auto_categorize_opted_in(env.clone()).await;
        output(saved)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListRulePresetsArgs {}

business_tool!(
    ListRulePresets,
    ListRulePresetsArgs,
    "list_rule_presets",
    "List available built-in categorization rule presets.",
    json!({"type":"object","properties":{},"required":[]}),
    [AgentScope::ClassificationRead],
    Read,
    |env, args| {
        let _ = args;
        output(env.categorization_rules_service().list_presets().await?)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportRulePresetArgs {
    pub preset_id: String,
}

business_tool!(ImportRulePreset, ImportRulePresetArgs, "import_rule_preset",
    "Import or upgrade a built-in rule preset using current taxonomy category keys. Existing user-modified rules are preserved by the service.",
    json!({"type":"object","properties":{"presetId":{"type":"string"}},"required":["presetId"]}), [AgentScope::ClassificationSuggest, AgentScope::ClassificationWrite], Write, |env, args| {
        let taxonomies = env.taxonomy_service().get_taxonomies_with_categories()?;
        let resolver = taxonomies.into_iter().filter(|t| t.taxonomy.scope == "activity").flat_map(|t| t.categories).map(|c| (c.key, (c.taxonomy_id, c.id))).collect();
        let saved = env.categorization_rules_service().import_preset(&args.preset_id, &resolver).await?;
        auto_categorize_opted_in(env.clone()).await;
        output(saved)
    });

pub fn tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(GetTaxonomies),
        Arc::new(GetTaxonomy),
        Arc::new(CreateTaxonomy),
        Arc::new(UpdateTaxonomy),
        Arc::new(CreateCategory),
        Arc::new(UpdateCategory),
        Arc::new(MoveCategory),
        Arc::new(ImportTaxonomyJson),
        Arc::new(AssignAssetToCategory),
        Arc::new(ReplaceAssetTaxonomyAssignments),
        Arc::new(UpdateCategorizationRule),
        Arc::new(UpsertCategorizationRule),
        Arc::new(ListRulePresets),
        Arc::new(ImportRulePreset),
    ]
}
