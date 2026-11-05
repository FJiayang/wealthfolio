use super::business_support::*;
use crate::{
    AgentEnvironment, AgentScope, AgentTool, AgentToolAccess, AgentToolError, AgentToolResult,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use wealthfolio_core::trade_instructions::{
    CancelInstruction, CompleteInstruction, CorrectCompletionTime, InstructionFilter,
    InstructionSource, InstructionVersion, NewTradeInstruction, UpdateTradeInstruction,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateArgs {
    input: NewTradeInstruction,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateArgs {
    id: String,
    input: UpdateTradeInstruction,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetArgs {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionArgs {
    id: String,
    input: InstructionVersion,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompleteArgs {
    id: String,
    input: CompleteInstruction,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelArgs {
    id: String,
    input: CancelInstruction,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CorrectArgs {
    id: String,
    input: CorrectCompletionTime,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    #[serde(default)]
    filter: InstructionFilter,
}

fn instruction_schema(edit: bool) -> Value {
    let asset = json!({"type":"object","additionalProperties":false,"properties":{
        "id":{"type":"string"},"symbol":{"type":"string"},"exchangeMic":{"type":"string"},
        "kind":{"type":"string"},"name":{"type":"string"},"quoteMode":{"type":"string","enum":["MARKET","MANUAL"]},
        "quoteCcy":{"type":"string"},"instrumentType":{"type":"string"},"providerId":{"type":"string"},"providerSymbol":{"type":"string"}
    }});
    let mut input = json!({"type":"object","additionalProperties":false,
        "properties":{
            "accountId":{"type":"string","minLength":1},"asset":asset,
            "side":{"type":"string","enum":["BUY","SELL","TRANSFER","DEPOSIT","WITHDRAWAL","CONVERSION"]},
            "quantity":{"type":"string"},"amount":{"type":"string"},"amountCurrency":{"type":"string"},
            "limitPrice":{"type":"string"},"notes":{"type":"string","maxLength":8000},
            "expectedCompletionAt":{"type":"string","format":"date-time"},
            "transfer":{"type":"object","additionalProperties":false,"required":["mode","isExternal"],"properties":{
                "mode":{"type":"string","enum":["cash","securities"]},"isExternal":{"type":"boolean"},
                "direction":{"type":"string","enum":["in","out"]},"toAccountId":{"type":"string","minLength":1},
                "destinationCurrency":{"type":"string"},"destinationAmount":{"type":"string"},
                "unitPrice":{"type":"string"},"costCurrency":{"type":"string"}
            },"allOf":[
                {"if":{"properties":{"isExternal":{"const":true}}},"then":{"required":["direction"],"not":{"anyOf":[{"required":["toAccountId"]},{"required":["destinationCurrency"]},{"required":["destinationAmount"]}]}},"else":{"required":["toAccountId"],"not":{"required":["direction"]}}},
                {"if":{"properties":{"mode":{"const":"cash"},"isExternal":{"const":false}}},"then":{"required":["destinationCurrency"]}},
                {"if":{"properties":{"mode":{"const":"securities"},"isExternal":{"const":true},"direction":{"const":"in"}},"required":["direction"]},"then":{"oneOf":[{"required":["unitPrice","costCurrency"]},{"not":{"anyOf":[{"required":["unitPrice"]},{"required":["costCurrency"]}]}}]},"else":{"not":{"anyOf":[{"required":["unitPrice"]},{"required":["costCurrency"]}]}}}
            ]},
            "conversion":{"type":"object","additionalProperties":false,"required":["toAsset"],"properties":{"toAsset":asset,"toQuantity":{"type":"string"}}}
        },
        "required":["accountId","side"],
        "oneOf":[{"required":["quantity"],"not":{"required":["amount"]}},{"required":["amount","amountCurrency"],"not":{"required":["quantity"]}}],
        "allOf":[
            {"if":{"properties":{"side":{"enum":["BUY","SELL"]}}},"then":{"required":["asset"]},"else":{"not":{"required":["limitPrice"]}}},
            {"if":{"properties":{"side":{"enum":["DEPOSIT","WITHDRAWAL"]}}},"then":{"required":["amount","amountCurrency"],"not":{"required":["asset"]}}},
            {"if":{"properties":{"side":{"const":"TRANSFER"}}},"then":{"required":["transfer"],"allOf":[
                {"if":{"properties":{"transfer":{"properties":{"mode":{"const":"cash"}}}}},"then":{"required":["amount","amountCurrency"],"not":{"required":["asset"]}},"else":{"required":["asset","quantity"]}}
            ]},"else":{"not":{"required":["transfer"]}}},
            {"if":{"properties":{"side":{"const":"CONVERSION"}}},"then":{"required":["asset","quantity","conversion"]},"else":{"not":{"required":["conversion"]}}}
        ]
    });
    let properties = input["properties"]
        .as_object_mut()
        .expect("schema properties");
    if edit {
        properties.insert(
            "expectedVersion".into(),
            json!({"type":"integer","minimum":1}),
        );
        input["required"]
            .as_array_mut()
            .expect("schema requirements")
            .push(json!("expectedVersion"));
    } else {
        properties.insert("goalId".into(), json!({"type":"string","minLength":1}));
        properties.insert("requestId".into(), json!({"type":"string","format":"uuid"}));
        input["required"]
            .as_array_mut()
            .expect("schema requirements")
            .extend([json!("goalId"), json!("requestId")]);
    }
    if edit {
        json!({"type":"object","additionalProperties":false,"required":["id","input"],"properties":{"id":{"type":"string","minLength":1},"input":input}})
    } else {
        json!({"type":"object","additionalProperties":false,"required":["input"],"properties":{"input":input}})
    }
}

business_tool!(CreateTradeInstruction, CreateArgs, "create_trade_instruction",
    "Save a BUY, SELL, TRANSFER, DEPOSIT, WITHDRAWAL or CONVERSION instruction under an existing goal for a human to execute externally. Cash operations have no asset. TRANSFER uses accountId as the source account internally or the local account externally; transfer contains mode, isExternal, and toAccountId or direction. Internal cash transfers specify both currencies, with optional destinationAmount; same-currency amounts match. Securities transfers require quantity; optional unitPrice and costCurrency apply only to external securities in. CONVERSION exchanges two distinct investment assets in one account: asset/quantity leave, conversion.toAsset arrives with optional toQuantity. Incoming amounts, units and cost may remain unknown. No balance or holdings check. May register an asset using manual-entry rules. Does NOT place an order or record an activity. Reuse requestId when retrying. Quantity and amount are mutually exclusive decimal strings. limitPrice is a buy ceiling/sell floor in the asset quote currency; absent means trader discretion. expectedCompletionAt is an optional soft deadline with timezone.",
    instruction_schema(false),
    [AgentScope::TradeInstructionsWrite, AgentScope::TradeInstructionsRead, AgentScope::AccountsRead, AgentScope::AssetsWrite, AgentScope::FinancialPlanningRead], Write, |env, args| {
        output(env.trade_instruction_service().create(args.input, InstructionSource::Mcp).await?)
    });
business_tool!(UpdateTradeInstructionTool, UpdateArgs, "update_trade_instruction",
    "Replace the editable content of a PENDING instruction of any of the six operation types. Use the same operation-specific fields as create_trade_instruction; omit irrelevant fields when changing type. Read the instruction first and pass its version as expectedVersion. Send the full editable content: omitted optional fields are cleared, not preserved. Identity, goal, source, creation time and original creation idempotency key stay unchanged. Unchanged assets should be sent as asset.id to preserve their snapshot. May register a changed asset under the same manual-entry rules as creation. Stale versions or non-PENDING instructions fail; reread before deciding whether to retry. This tool does NOT place orders, create activities or change execution status. ",
    instruction_schema(true),
    [AgentScope::TradeInstructionsWrite, AgentScope::TradeInstructionsRead, AgentScope::AccountsRead, AgentScope::AssetsWrite], Write, |env, args| {
        output(env.trade_instruction_service().edit(&args.id, args.input).await?)
    });
business_tool!(GetTradeInstruction, GetArgs, "get_trade_instruction",
    "Read one trade instruction and its manually reported status. Completion is not broker verification and does not imply bookkeeping.",
    json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false}),
    [AgentScope::TradeInstructionsRead], Read, |env, args| { output(env.trade_instruction_service().get(&args.id)?) });
business_tool!(ListTradeInstructions, ListArgs, "list_trade_instructions",
    "List locally stored trade instructions, newest first unless sort is specified. Expected completion bounds are inclusive from and exclusive before. Missing expected times sort last. Use side for one type or sides for multiple, never both; empty sides includes all types. accountId matches either internal transfer endpoint without duplicate rows. Default statuses are PENDING and IN_PROGRESS; an empty statuses array includes all. No provider requests.",
    json!({"type":"object","additionalProperties":false,"properties":{"filter":{"type":"object","additionalProperties":false,"properties":{"goalId":{"type":"string"},"statuses":{"type":"array","items":{"type":"string","enum":["PENDING","IN_PROGRESS","COMPLETED","CANCELLED"]}},"accountId":{"type":"string"},"side":{"type":"string","enum":["BUY","SELL","TRANSFER","DEPOSIT","WITHDRAWAL","CONVERSION"]},"sides":{"type":"array","items":{"type":"string","enum":["BUY","SELL","TRANSFER","DEPOSIT","WITHDRAWAL","CONVERSION"]}},"expectedCompletionFrom":{"type":"string","format":"date-time"},"expectedCompletionBefore":{"type":"string","format":"date-time"},"sort":{"type":"string","enum":["expectedCompletionAsc","expectedCompletionDesc"]},"limit":{"type":"integer","minimum":1,"maximum":100,"default":50},"offset":{"type":"integer","minimum":0}}}}}),
    [AgentScope::TradeInstructionsRead], Read, |env, args| { output(env.trade_instruction_service().list(args.filter)?) });
business_tool!(StartTradeInstruction, VersionArgs, "start_trade_instruction",
    "Start a PENDING instruction. Read it first and pass its version as expectedVersion. Records local execution status only; does not place an order or create bookkeeping activities.",
    json!({"type":"object","additionalProperties":false,"required":["id","input"],"properties":{"id":{"type":"string"},"input":{"type":"object","additionalProperties":false,"required":["expectedVersion"],"properties":{"expectedVersion":{"type":"integer","minimum":1}}}}}),
    [AgentScope::TradeInstructionsWrite, AgentScope::TradeInstructionsRead], Write, |env, args| { output(env.trade_instruction_service().start(&args.id, args.input.expected_version).await?) });
business_tool!(CompleteTradeInstruction, CompleteArgs, "complete_trade_instruction",
    "Mark a PENDING or IN_PROGRESS instruction completed. Requires the version read from the record. resolutionNote is optional; absent completedAt uses current server time. Explicit completedAt must include a timezone and cannot be in the future. This is a reported completion, not broker verification, and creates no bookkeeping activity.",
    json!({"type":"object","additionalProperties":false,"required":["id","input"],"properties":{"id":{"type":"string"},"input":{"type":"object","additionalProperties":false,"required":["expectedVersion"],"properties":{"expectedVersion":{"type":"integer","minimum":1},"resolutionNote":{"type":["string","null"],"maxLength":4000},"completedAt":{"type":"string","format":"date-time"}}}}}),
    [AgentScope::TradeInstructionsWrite, AgentScope::TradeInstructionsRead], Write, |env, args| { output(env.trade_instruction_service().complete(&args.id, args.input).await?) });
business_tool!(CancelTradeInstruction, CancelArgs, "cancel_trade_instruction",
    "Cancel a PENDING or IN_PROGRESS instruction using its current expectedVersion and a required nonblank resolutionNote. Does not cancel an actual broker order or create bookkeeping activities.",
    json!({"type":"object","additionalProperties":false,"required":["id","input"],"properties":{"id":{"type":"string"},"input":{"type":"object","additionalProperties":false,"required":["expectedVersion","resolutionNote"],"properties":{"expectedVersion":{"type":"integer","minimum":1},"resolutionNote":{"type":"string","minLength":1,"maxLength":4000}}}}}),
    [AgentScope::TradeInstructionsWrite, AgentScope::TradeInstructionsRead], Write, |env, args| { output(env.trade_instruction_service().cancel(&args.id, args.input).await?) });
business_tool!(CorrectTradeInstructionCompletionTime, CorrectArgs, "correct_trade_instruction_completion_time",
    "Correct actual completedAt of a COMPLETED instruction using its current expectedVersion. The time must include a timezone and cannot be in the future. Preserves the original system closure time and creates no bookkeeping activity.",
    json!({"type":"object","additionalProperties":false,"required":["id","input"],"properties":{"id":{"type":"string"},"input":{"type":"object","additionalProperties":false,"required":["expectedVersion","completedAt"],"properties":{"expectedVersion":{"type":"integer","minimum":1},"completedAt":{"type":"string","format":"date-time"}}}}}),
    [AgentScope::TradeInstructionsWrite, AgentScope::TradeInstructionsRead], Write, |env, args| { output(env.trade_instruction_service().correct_completion_time(&args.id, args.input).await?) });
business_tool!(DeleteTradeInstruction, VersionArgs, "delete_trade_instruction",
    "Permanently delete an instruction of any status using its current expectedVersion. Cannot be undone. Removes the instruction and its creation requestId deduplication key only; does not delete accounts, assets or activities or cancel actual broker orders.",
    json!({"type":"object","additionalProperties":false,"required":["id","input"],"properties":{"id":{"type":"string"},"input":{"type":"object","additionalProperties":false,"required":["expectedVersion"],"properties":{"expectedVersion":{"type":"integer","minimum":1}}}}}),
    [AgentScope::TradeInstructionsWrite, AgentScope::TradeInstructionsRead], Write, |env, args| {
        env.trade_instruction_service().delete(&args.id, args.input.expected_version).await?;
        output(json!({"id":args.id,"deleted":true}))
    });
pub fn tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(CreateTradeInstruction),
        Arc::new(UpdateTradeInstructionTool),
        Arc::new(StartTradeInstruction),
        Arc::new(CompleteTradeInstruction),
        Arc::new(CancelTradeInstruction),
        Arc::new(CorrectTradeInstructionCompletionTime),
        Arc::new(DeleteTradeInstruction),
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
        assert!(mcp.get("complete_trade_instruction").is_some());
    }
    #[test]
    fn instruction_mutations_share_write_permission_and_redact_arguments() {
        let schema = UpdateTradeInstructionTool.input_schema();
        let input = &schema["properties"]["input"];
        assert!(input["required"]
            .as_array()
            .unwrap()
            .contains(&json!("expectedVersion")));
        for immutable in [
            "id",
            "goalId",
            "requestId",
            "source",
            "status",
            "completedAt",
            "createdAt",
        ] {
            assert!(input["properties"].get(immutable).is_none());
        }
        assert_eq!(schema["properties"]["input"]["additionalProperties"], false);
        let scopes = UpdateTradeInstructionTool.required_scopes();
        assert!(scopes.contains(&AgentScope::TradeInstructionsWrite));
        assert!(!scopes.contains(&AgentScope::ActivitiesWrite));
        assert!(!AgentScopeSet::read_only().grants_all(scopes));
        assert!(AgentScopeSet::from_strs([
            "trade-instructions:create",
            "trade-instructions:read",
            "accounts:read",
            "assets:write",
            "financial-planning:read"
        ])
        .grants_all(scopes));
        assert_eq!(
            UpdateTradeInstructionTool
                .sanitize_args_for_audit(&json!({"id":"private","input":{"notes":"private"}})),
            Value::Null
        );
        let mcp = AgentToolCatalog::mcp_catalog();
        assert!(mcp.get("update_trade_instruction").is_some());
        for operation in [
            "start_trade_instruction",
            "complete_trade_instruction",
            "cancel_trade_instruction",
            "correct_trade_instruction_completion_time",
            "delete_trade_instruction",
        ] {
            let tool = mcp.get(operation).unwrap();
            assert!(tool
                .required_scopes()
                .contains(&AgentScope::TradeInstructionsWrite));
            assert!(!AgentScopeSet::read_only().grants_all(tool.required_scopes()));
            assert_eq!(
                tool.sanitize_args_for_audit(&json!({"input":{"resolutionNote":"private"}})),
                Value::Null
            );
        }
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

    #[test]
    fn instruction_schemas_expose_actions_and_optional_incoming_values() {
        for schema in [
            CreateTradeInstruction.input_schema(),
            UpdateTradeInstructionTool.input_schema(),
        ] {
            let input = &schema["properties"]["input"];
            assert_eq!(
                input["properties"]["side"]["enum"],
                json!([
                    "BUY",
                    "SELL",
                    "TRANSFER",
                    "DEPOSIT",
                    "WITHDRAWAL",
                    "CONVERSION"
                ])
            );
            assert!(!input["required"]
                .as_array()
                .unwrap()
                .contains(&json!("asset")));
            let transfer = &input["properties"]["transfer"];
            assert_eq!(transfer["additionalProperties"], false);
            assert_eq!(transfer["required"], json!(["mode", "isExternal"]));
            assert!(!transfer["required"]
                .as_array()
                .unwrap()
                .contains(&json!("destinationAmount")));
            assert_eq!(
                input["properties"]["conversion"]["required"],
                json!(["toAsset"])
            );
            for value in ["quantity", "amount", "limitPrice"] {
                assert_eq!(input["properties"][value]["type"], "string");
            }
        }
        let list = ListTradeInstructions.input_schema();
        assert!(list["properties"]["filter"]["properties"]["sides"].is_object());
        assert!(list["properties"]["filter"]["properties"]["side"].is_object());
    }
}
