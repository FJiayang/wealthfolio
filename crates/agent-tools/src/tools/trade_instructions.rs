use super::business_support::*;
use crate::{
    AgentEnvironment, AgentScope, AgentTool, AgentToolAccess, AgentToolError, AgentToolResult,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use wealthfolio_core::trade_instructions::{
    InstructionFilter, InstructionSource, NewTradeInstruction,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateArgs {
    input: NewTradeInstruction,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetArgs {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    #[serde(default)]
    filter: InstructionFilter,
}

business_tool!(CreateTradeInstruction, CreateArgs, "create_trade_instruction",
    "Save a trade instruction under an existing goal for a human to execute externally. May register an asset using manual-entry rules. Does NOT place an order or record an activity. Reuse requestId when retrying. Only humans can start, complete, cancel or correct completion time. Quantity and amount are mutually exclusive decimal strings. limitPrice is a buy ceiling/sell floor in the asset quote currency; absent means trader discretion. expectedCompletionAt is an optional soft deadline with timezone.",
    json!({"type":"object","additionalProperties":false,"properties":{"input":{"type":"object","additionalProperties":false,"properties":{
        "goalId":{"type":"string","minLength":1},"requestId":{"type":"string","format":"uuid"},"accountId":{"type":"string"},
        "asset":{"type":"object","properties":{"id":{"type":"string"},"symbol":{"type":"string"},"exchangeMic":{"type":"string"},"kind":{"type":"string"},"name":{"type":"string"},"quoteMode":{"type":"string","enum":["MARKET","MANUAL"]},"quoteCcy":{"type":"string"},"instrumentType":{"type":"string"},"providerId":{"type":"string"},"providerSymbol":{"type":"string"}}},
        "side":{"type":"string","enum":["BUY","SELL"]},"quantity":{"type":"string"},"amount":{"type":"string"},"amountCurrency":{"type":"string"},"limitPrice":{"type":"string"},"notes":{"type":"string","maxLength":8000,"description":"Optional free-text notes, including reasons or execution details."},"expectedCompletionAt":{"type":"string","format":"date-time"}
    },"required":["goalId","requestId","accountId","asset","side"],"oneOf":[{"required":["quantity"],"not":{"required":["amount"]}},{"required":["amount","amountCurrency"],"not":{"required":["quantity"]}}]}},"required":["input"]}),
    [AgentScope::TradeInstructionsCreate, AgentScope::TradeInstructionsRead, AgentScope::AccountsRead, AgentScope::AssetsWrite, AgentScope::FinancialPlanningRead], Write, |env, args| {
        output(env.trade_instruction_service().create(args.input, InstructionSource::Mcp).await?)
    });
business_tool!(GetTradeInstruction, GetArgs, "get_trade_instruction",
    "Read one trade instruction and its manually reported status. Completion is not broker verification and does not imply bookkeeping.",
    json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false}),
    [AgentScope::TradeInstructionsRead], Read, |env, args| { output(env.trade_instruction_service().get(&args.id)?) });
business_tool!(ListTradeInstructions, ListArgs, "list_trade_instructions",
    "List locally stored trade instructions, newest first unless sort is specified. Expected completion bounds are inclusive from and exclusive before. Missing expected times sort last. Default statuses are PENDING and IN_PROGRESS; an empty statuses array includes all. No provider requests.",
    json!({"type":"object","additionalProperties":false,"properties":{"filter":{"type":"object","additionalProperties":false,"properties":{"goalId":{"type":"string"},"statuses":{"type":"array","items":{"type":"string","enum":["PENDING","IN_PROGRESS","COMPLETED","CANCELLED"]}},"accountId":{"type":"string"},"side":{"type":"string","enum":["BUY","SELL"]},"expectedCompletionFrom":{"type":"string","format":"date-time"},"expectedCompletionBefore":{"type":"string","format":"date-time"},"sort":{"type":"string","enum":["expectedCompletionAsc","expectedCompletionDesc"]},"limit":{"type":"integer","minimum":1,"maximum":100,"default":50},"offset":{"type":"integer","minimum":0}}}}}),
    [AgentScope::TradeInstructionsRead], Read, |env, args| { output(env.trade_instruction_service().list(args.filter)?) });
pub fn tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(CreateTradeInstruction),
        Arc::new(GetTradeInstruction),
        Arc::new(ListTradeInstructions),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentScopeSet, AgentToolCatalog};
    #[test]
    fn tools_are_mcp_only_and_creation_is_not_bookkeeping() {
        let mcp = AgentToolCatalog::mcp_catalog();
        let assistant = AgentToolCatalog::assistant_catalog();
        for tool in tools() {
            assert!(mcp.get(tool.name()).is_some());
            assert!(assistant.get(tool.name()).is_none());
        }
        let scopes = CreateTradeInstruction.required_scopes();
        assert!(!scopes.contains(&AgentScope::ActivitiesWrite));
        assert!(!AgentScopeSet::read_only().grants_all(scopes));
        assert_eq!(
            CreateTradeInstruction.sanitize_args_for_audit(&json!({"quantity":"123"})),
            Value::Null
        );
        assert!(mcp.get("complete_trade_instruction").is_none());
    }
    #[test]
    fn creation_requires_goal_and_exposes_only_optional_notes() {
        let schema = CreateTradeInstruction.input_schema();
        let input = &schema["properties"]["input"];
        assert!(input["required"]
            .as_array()
            .unwrap()
            .contains(&json!("goalId")));
        assert!(input["properties"]["notes"].is_object());
        assert!(input["properties"].get("rationale").is_none());
        assert!(!input["required"]
            .as_array()
            .unwrap()
            .contains(&json!("notes")));
        assert_eq!(
            CreateTradeInstruction.sanitize_args_for_audit(&json!({"input":{"notes":"private"}})),
            Value::Null
        );
    }
}
