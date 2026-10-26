use crate::profiles::ProfileAccess;
use wealthfolio_core::trade_instructions::*;

#[tauri::command]
pub async fn create_trade_instruction(
    input: NewTradeInstruction,
    state: ProfileAccess,
) -> Result<TradeInstruction, String> {
    state
        .context()?
        .trade_instruction_service()
        .create(input, InstructionSource::Ui)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_trade_instructions(
    filter: InstructionFilter,
    state: ProfileAccess,
) -> Result<InstructionPage, String> {
    state
        .context()?
        .trade_instruction_service()
        .list(filter)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_trade_instruction(
    id: String,
    state: ProfileAccess,
) -> Result<TradeInstruction, String> {
    state
        .context()?
        .trade_instruction_service()
        .get(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_trade_instruction(
    id: String,
    input: InstructionVersion,
    state: ProfileAccess,
) -> Result<TradeInstruction, String> {
    state
        .context()?
        .trade_instruction_service()
        .start(&id, input.expected_version)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn complete_trade_instruction(
    id: String,
    input: CompleteInstruction,
    state: ProfileAccess,
) -> Result<TradeInstruction, String> {
    state
        .context()?
        .trade_instruction_service()
        .complete(&id, input)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cancel_trade_instruction(
    id: String,
    input: CancelInstruction,
    state: ProfileAccess,
) -> Result<TradeInstruction, String> {
    state
        .context()?
        .trade_instruction_service()
        .cancel(&id, input)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn correct_trade_instruction_completion_time(
    id: String,
    input: CorrectCompletionTime,
    state: ProfileAccess,
) -> Result<TradeInstruction, String> {
    state
        .context()?
        .trade_instruction_service()
        .correct_completion_time(&id, input)
        .await
        .map_err(|e| e.to_string())
}
