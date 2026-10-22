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
struct GetActivityArgs {
    pub activity_id: String,
}

business_tool!(
    GetActivity,
    GetActivityArgs,
    "get_activity",
    "Read a complete activity record by ID.",
    json!({"type":"object","properties":{"activityId":{"type":"string"}},"required":["activityId"]}),
    [AgentScope::ActivitiesRead],
    Read,
    |env, args| { output(env.activity_service().get_activity(&args.activity_id)?) }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetTransferPairForActivityArgs {
    pub activity_id: String,
}

business_tool!(
    GetTransferPairForActivity,
    GetTransferPairForActivityArgs,
    "get_transfer_pair_for_activity",
    "Read the internal transfer pair associated with an activity.",
    json!({"type":"object","properties":{"activityId":{"type":"string"}},"required":["activityId"]}),
    [AgentScope::ActivitiesRead],
    Read,
    |env, args| {
        output(
            env.activity_service()
                .get_transfer_pair_for_activity(args.activity_id)?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FindTransferMatchCandidatesArgs {
    pub request: wealthfolio_core::activities::TransferMatchCandidateRequest,
}

business_tool!(
    FindTransferMatchCandidates,
    FindTransferMatchCandidatesArgs,
    "find_transfer_match_candidates",
    "Read potential matches for one transfer activity using existing scoring.",
    json!({"type":"object","properties":{"request":{"type":"object","properties":{"activityId":{"type":"string"},"windowDays":{"type":"integer"},"limit":{"type":"integer"}},"required":["activityId"]}},"required":["request"]}),
    [AgentScope::ActivitiesRead],
    Read,
    |env, args| {
        output(
            env.activity_service()
                .find_transfer_match_candidates(args.request)?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveInternalTransferPairArgs {
    pub request: wealthfolio_core::activities::InternalTransferPairRequest,
}

business_tool!(SaveInternalTransferPair, SaveInternalTransferPairArgs, "save_internal_transfer_pair",
    "Create or update both legs of an internal cash transfer through the same validated service as the activity form. Does not place broker orders.",
    json!({"type":"object","properties":{"request":{"type":"object","properties":{"editedActivityId":{"type":"string"},"transferOutId":{"type":"string"},"transferInId":{"type":"string"},"sourceGroupId":{"type":"string"},"fromAccountId":{"type":"string"},"toAccountId":{"type":"string"},"activityDate":{"type":"string"},"sourceAmount":{"oneOf":[{"type":"string"},{"type":"number"}]},"destinationAmount":{"oneOf":[{"type":"string"},{"type":"number"}]},"sourceCurrency":{"type":"string"},"destinationCurrency":{"type":"string"},"fxRate":{"oneOf":[{"type":"string"},{"type":"number"}]},"notes":{"type":"string"},"transferMode":{"type":"string"}},"required":["fromAccountId","toAccountId","activityDate","sourceCurrency","destinationCurrency"]}},"required":["request"]}), [AgentScope::ActivitiesDraft, AgentScope::ActivitiesWrite], Write, |env, args| {
        output(env.activity_service().save_internal_transfer_pair(args.request).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListImportTemplatesArgs {}

business_tool!(ListImportTemplates, ListImportTemplatesArgs, "list_import_templates",
    "Read saved import templates or account mappings. Does not import activities or manage broker synchronization.",
    json!({"type":"object","properties":{},"required":[]}), [AgentScope::ActivitiesRead], Read, |env, args| {
        let _ = args; output(env.activity_service().list_import_templates()?.into_iter().filter(|t| matches!(t.kind, wealthfolio_core::activities::TemplateKind::CsvActivity | wealthfolio_core::activities::TemplateKind::CsvHoldings)).collect::<Vec<_>>())
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetImportTemplateArgs {
    pub id: String,
}

business_tool!(GetImportTemplate, GetImportTemplateArgs, "get_import_template",
    "Read saved import templates or account mappings. Does not import activities or manage broker synchronization.",
    json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}), [AgentScope::ActivitiesRead], Read, |env, args| {
        let template = env.activity_service().get_import_template(args.id)?;
        if !matches!(template.kind, wealthfolio_core::activities::TemplateKind::CsvActivity | wealthfolio_core::activities::TemplateKind::CsvHoldings) { return Err(AgentToolError::InvalidInput("Only CSV import templates are available".into())); }
        output(template)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetAccountImportMappingArgs {
    pub account_id: String,
    pub context_kind: String,
}

business_tool!(GetAccountImportMapping, GetAccountImportMappingArgs, "get_account_import_mapping",
    "Read saved import templates or account mappings. Does not import activities or manage broker synchronization.",
    json!({"type":"object","properties":{"accountId":{"type":"string"},"contextKind":{"type":"string","enum":["CSV_ACTIVITY","CSV_HOLDINGS"]}},"required":["accountId","contextKind"]}), [AgentScope::ActivitiesRead], Read, |env, args| {
        if !matches!(args.context_kind.as_str(), "CSV_ACTIVITY" | "CSV_HOLDINGS") { return Err(AgentToolError::InvalidInput("Only CSV import contexts can be read".into())); }
        output(env.activity_service().get_import_mapping(args.account_id, args.context_kind)?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveImportTemplateArgs {
    pub input: wealthfolio_core::activities::ImportTemplateData,
}

business_tool!(SaveImportTemplate, SaveImportTemplateArgs, "save_import_template",
    "Save CSV import configuration through existing validation. Broker synchronization profiles and connection configuration are excluded.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"name":{"type":"string"},"fieldMappings":{"type":"object","additionalProperties":{"oneOf":[{"type":"string"},{"type":"array","items":{"type":"string"}}]}},"activityMappings":{"type":"object","additionalProperties":{"type":"array","items":{"type":"string"}}},"symbolMappings":{"type":"object","additionalProperties":{"type":"string"}},"accountMappings":{"type":"object","additionalProperties":{"type":"string"}},"symbolMappingMeta":{"type":"object","additionalProperties":{"type":"object","properties":{"exchangeMic":{"type":"string"},"symbolName":{"type":"string"},"quoteCcy":{"type":"string"},"instrumentType":{"type":"string"},"quoteMode":{"type":"string"}}}},"parseConfig":{"type":"object","properties":{"delimiter":{"type":"string"},"quoteChar":{"type":"string"},"dateFormat":{"type":"string"},"decimalSeparator":{"type":"string"},"thousandsSeparator":{"type":"string"},"defaultCurrency":{"type":"string"},"headerRowIndex":{"type":"integer"},"skipTopRows":{"type":"integer"},"skipBottomRows":{"type":"integer"},"hasHeaderRow":{"type":"boolean"},"skipEmptyRows":{"type":"boolean"}}},"id":{"type":"string"},"scope":{"type":"string","enum":["SYSTEM","USER"]},"kind":{"type":"string","enum":["CSV_ACTIVITY","CSV_HOLDINGS"]}},"required":["id","name","scope"]}},"required":["input"]}), [AgentScope::ActivitiesDraft, AgentScope::ActivitiesWrite], Write, |env, args| {
        if !matches!(args.input.kind, wealthfolio_core::activities::TemplateKind::CsvActivity | wealthfolio_core::activities::TemplateKind::CsvHoldings) { return Err(AgentToolError::InvalidInput("Only CSV import templates can be saved".into())); }
        output(env.activity_service().save_import_template(args.input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveAccountImportMappingArgs {
    pub input: wealthfolio_core::activities::ImportMappingData,
}

business_tool!(SaveAccountImportMapping, SaveAccountImportMappingArgs, "save_account_import_mapping",
    "Save CSV import configuration through existing validation. Broker synchronization profiles and connection configuration are excluded.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"accountId":{"type":"string"},"contextKind":{"type":"string","enum":["CSV_ACTIVITY","CSV_HOLDINGS"]},"name":{"type":"string"},"templateId":{"type":"string"},"fieldMappings":{"type":"object","additionalProperties":{"oneOf":[{"type":"string"},{"type":"array","items":{"type":"string"}}]}},"activityMappings":{"type":"object","additionalProperties":{"type":"array","items":{"type":"string"}}},"symbolMappings":{"type":"object","additionalProperties":{"type":"string"}},"accountMappings":{"type":"object","additionalProperties":{"type":"string"}},"symbolMappingMeta":{"type":"object","additionalProperties":{"type":"object","properties":{"exchangeMic":{"type":"string"},"symbolName":{"type":"string"},"quoteCcy":{"type":"string"},"instrumentType":{"type":"string"},"quoteMode":{"type":"string"}}}},"parseConfig":{"type":"object","properties":{"delimiter":{"type":"string"},"quoteChar":{"type":"string"},"dateFormat":{"type":"string"},"decimalSeparator":{"type":"string"},"thousandsSeparator":{"type":"string"},"defaultCurrency":{"type":"string"},"headerRowIndex":{"type":"integer"},"skipTopRows":{"type":"integer"},"skipBottomRows":{"type":"integer"},"hasHeaderRow":{"type":"boolean"},"skipEmptyRows":{"type":"boolean"}}}},"required":["accountId"]}},"required":["input"]}), [AgentScope::ActivitiesDraft, AgentScope::ActivitiesWrite], Write, |env, args| {
        if !matches!(args.input.context_kind.as_str(), "CSV_ACTIVITY" | "CSV_HOLDINGS") { return Err(AgentToolError::InvalidInput("Only CSV import contexts are supported".into())); }
        output(env.activity_service().save_import_mapping(args.input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LinkAccountTemplateArgs {
    pub account_id: String,
    pub template_id: String,
    pub context_kind: String,
}

business_tool!(LinkAccountTemplate, LinkAccountTemplateArgs, "link_account_template",
    "Link an existing CSV template to an account and import context. Does not change broker synchronization profiles.",
    json!({"type":"object","properties":{"accountId":{"type":"string"},"templateId":{"type":"string"},"contextKind":{"type":"string","enum":["CSV_ACTIVITY","CSV_HOLDINGS"]}},"required":["accountId","templateId","contextKind"]}), [AgentScope::ActivitiesDraft, AgentScope::ActivitiesWrite], Write, |env, args| {
        if !matches!(args.context_kind.as_str(), "CSV_ACTIVITY" | "CSV_HOLDINGS") { return Err(AgentToolError::InvalidInput("Only CSV import contexts are supported".into())); }
        let template = env.activity_service().get_import_template(args.template_id.clone())?;
        if !matches!(template.kind, wealthfolio_core::activities::TemplateKind::CsvActivity | wealthfolio_core::activities::TemplateKind::CsvHoldings) { return Err(AgentToolError::InvalidInput("Only CSV templates can be linked".into())); }
        env.activity_service().link_account_template(args.account_id, args.template_id, args.context_kind).await?;
        output(json!({"saved": true}))
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CheckExistingDuplicatesArgs {
    pub idempotency_keys: Vec<String>,
}

business_tool!(
    CheckExistingDuplicates,
    CheckExistingDuplicatesArgs,
    "check_existing_duplicates",
    "Check activity idempotency keys against existing saved activities without writing.",
    json!({"type":"object","properties":{"idempotencyKeys":{"type":"array","items":{"type":"string"},"maxItems":100}},"required":["idempotencyKeys"]}),
    [AgentScope::ActivitiesRead],
    Read,
    |env, args| {
        if args.idempotency_keys.len() > 100 {
            return Err(AgentToolError::InvalidInput(
                "At most 100 items per call".into(),
            ));
        }
        output(
            env.activity_service()
                .check_existing_duplicates(args.idempotency_keys)?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreviewImportAssetsArgs {
    pub candidates: Vec<wealthfolio_core::activities::ImportAssetCandidate>,
}

business_tool!(PreviewImportAssets, PreviewImportAssetsArgs, "preview_import_assets",
    "Resolve and validate asset candidates through the existing import preview service, without saving activities or assets. Uses only configured providers.",
    json!({"type":"object","properties":{"candidates":{"type":"array","items":{"type":"object","properties":{"key":{"type":"string"},"accountId":{"type":"string"},"symbol":{"type":"string"},"currency":{"type":"string"},"instrumentType":{"type":"string"},"quoteCcy":{"type":"string"},"quoteMode":{"type":"string"},"exchangeMic":{"type":"string"},"isin":{"type":"string"},"providerId":{"type":"string"},"providerSymbol":{"type":"string"}},"required":["key","accountId","symbol"]},"maxItems":100}},"required":["candidates"]}), [AgentScope::ActivitiesRead], Read, |env, args| {
        if args.candidates.len() > 100 { return Err(AgentToolError::InvalidInput("At most 100 items per call".into())); }
        output(env.activity_service().preview_import_assets(args.candidates).await?)
    });

pub fn tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(GetActivity),
        Arc::new(GetTransferPairForActivity),
        Arc::new(FindTransferMatchCandidates),
        Arc::new(SaveInternalTransferPair),
        Arc::new(ListImportTemplates),
        Arc::new(GetImportTemplate),
        Arc::new(GetAccountImportMapping),
        Arc::new(SaveImportTemplate),
        Arc::new(SaveAccountImportMapping),
        Arc::new(LinkAccountTemplate),
        Arc::new(CheckExistingDuplicates),
        Arc::new(PreviewImportAssets),
    ]
}
