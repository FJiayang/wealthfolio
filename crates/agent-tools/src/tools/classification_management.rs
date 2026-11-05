//! MCP-only business tools. Domain services own validation and persistence.
use super::business_support::*;
use crate::{
    AgentEnvironment, AgentScope, AgentTool, AgentToolAccess, AgentToolError, AgentToolResult,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

fn require_asset_scope(scope: &str) -> Result<(), AgentToolError> {
    if scope != "asset" {
        return Err(AgentToolError::InvalidInput(
            "Only asset-scoped taxonomies can be maintained; use the Spending tools for transaction categories".into(),
        ));
    }
    Ok(())
}

fn asset_taxonomy(
    env: &dyn AgentEnvironment,
    id: &str,
) -> Result<wealthfolio_core::taxonomies::TaxonomyWithCategories, AgentToolError> {
    let taxonomy = env
        .taxonomy_service()
        .get_taxonomy(id)?
        .ok_or_else(|| AgentToolError::NotFound("Asset taxonomy not found".into()))?;
    require_asset_scope(&taxonomy.taxonomy.scope)?;
    Ok(taxonomy)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateTaxonomyArgs {
    pub input: wealthfolio_core::taxonomies::NewTaxonomy,
}

business_tool!(CreateTaxonomy, CreateTaxonomyArgs, "create_taxonomy",
    "Save asset taxonomy/category configuration through existing domain validation. No deletion or system migration is performed.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"id":{"type":"string"},"name":{"type":"string"},"color":{"type":"string"},"description":{"type":"string"},"isSystem":{"type":"boolean"},"isSingleSelect":{"type":"boolean"},"sortOrder":{"type":"integer"},"scope":{"type":"string","enum":["asset"]},"createdAt":{"type":"string"},"updatedAt":{"type":"string"}},"required":["name","color","isSystem","isSingleSelect","sortOrder"]}},"required":["input"]}), [AgentScope::ClassificationSuggest, AgentScope::ClassificationWrite], Write, |env, args| {
        require_asset_scope(&args.input.scope)?;
        if let Some(id) = &args.input.id {
            if let Some(existing) = env.taxonomy_service().get_taxonomy(id)? {
                require_asset_scope(&existing.taxonomy.scope)?;
            }
        }
        output(env.taxonomy_service().create_taxonomy(args.input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateTaxonomyArgs {
    pub input: wealthfolio_core::taxonomies::Taxonomy,
}

business_tool!(UpdateTaxonomy, UpdateTaxonomyArgs, "update_taxonomy",
    "Save asset taxonomy/category configuration through existing domain validation. No deletion or system migration is performed.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"id":{"type":"string"},"name":{"type":"string"},"color":{"type":"string"},"description":{"type":"string"},"isSystem":{"type":"boolean"},"isSingleSelect":{"type":"boolean"},"sortOrder":{"type":"integer"},"scope":{"type":"string","enum":["asset"]},"createdAt":{"type":"string"},"updatedAt":{"type":"string"}},"required":["name","color","isSystem","isSingleSelect","sortOrder","id","createdAt","updatedAt"]}},"required":["input"]}), [AgentScope::ClassificationSuggest, AgentScope::ClassificationWrite], Write, |env, args| {
        require_asset_scope(&args.input.scope)?;
        asset_taxonomy(env.as_ref(), &args.input.id)?;
        output(env.taxonomy_service().update_taxonomy(args.input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateCategoryArgs {
    pub input: wealthfolio_core::taxonomies::NewCategory,
}

business_tool!(CreateCategory, CreateCategoryArgs, "create_category",
    "Save asset taxonomy/category configuration through existing domain validation. No deletion or system migration is performed.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"id":{"type":"string"},"taxonomyId":{"type":"string"},"parentId":{"type":"string"},"name":{"type":"string"},"key":{"type":"string"},"color":{"type":"string"},"description":{"type":"string"},"sortOrder":{"type":"integer"},"icon":{"type":"string"},"createdAt":{"type":"string"},"updatedAt":{"type":"string"}},"required":["taxonomyId","name","key","color","sortOrder"]}},"required":["input"]}), [AgentScope::ClassificationSuggest, AgentScope::ClassificationWrite], Write, |env, args| {
        asset_taxonomy(env.as_ref(), &args.input.taxonomy_id)?;
        output(env.taxonomy_service().create_category(args.input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateCategoryArgs {
    pub input: wealthfolio_core::taxonomies::Category,
}

business_tool!(UpdateCategory, UpdateCategoryArgs, "update_category",
    "Save asset taxonomy/category configuration through existing domain validation. No deletion or system migration is performed.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"id":{"type":"string"},"taxonomyId":{"type":"string"},"parentId":{"type":"string"},"name":{"type":"string"},"key":{"type":"string"},"color":{"type":"string"},"description":{"type":"string"},"sortOrder":{"type":"integer"},"icon":{"type":"string"},"createdAt":{"type":"string"},"updatedAt":{"type":"string"}},"required":["taxonomyId","name","key","color","sortOrder","id","createdAt","updatedAt"]}},"required":["input"]}), [AgentScope::ClassificationSuggest, AgentScope::ClassificationWrite], Write, |env, args| {
        let taxonomy = asset_taxonomy(env.as_ref(), &args.input.taxonomy_id)?;
        if !taxonomy.categories.iter().any(|category| category.id == args.input.id) {
            return Err(AgentToolError::NotFound("Asset category not found".into()));
        }
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
        asset_taxonomy(env.as_ref(), &args.taxonomy_id)?;
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

pub fn tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(CreateTaxonomy),
        Arc::new(UpdateTaxonomy),
        Arc::new(CreateCategory),
        Arc::new(UpdateCategory),
        Arc::new(MoveCategory),
        Arc::new(ImportTaxonomyJson),
    ]
}
