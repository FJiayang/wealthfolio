use super::*;
use crate::Result;
use async_trait::async_trait;

#[async_trait]
pub trait TradeInstructionRepositoryTrait: Send + Sync {
    fn get(&self, id: &str) -> Result<TradeInstruction>;
    fn get_by_request_id(&self, request_id: &str) -> Result<Option<TradeInstruction>>;
    fn list(&self, filter: InstructionFilter) -> Result<InstructionPage>;
    async fn create(&self, instruction: TradeInstruction) -> Result<TradeInstruction>;
    async fn update(
        &self,
        instruction: TradeInstruction,
        expected_version: i32,
        expected_status: InstructionStatus,
    ) -> Result<TradeInstruction>;
}
#[async_trait]
pub trait TradeInstructionServiceTrait: Send + Sync {
    fn get(&self, id: &str) -> Result<TradeInstruction>;
    fn list(&self, filter: InstructionFilter) -> Result<InstructionPage>;
    async fn create(
        &self,
        input: NewTradeInstruction,
        source: InstructionSource,
    ) -> Result<TradeInstruction>;
    async fn start(&self, id: &str, expected_version: i32) -> Result<TradeInstruction>;
    async fn complete(&self, id: &str, input: CompleteInstruction) -> Result<TradeInstruction>;
    async fn cancel(&self, id: &str, input: CancelInstruction) -> Result<TradeInstruction>;
    async fn correct_completion_time(
        &self,
        id: &str,
        input: CorrectCompletionTime,
    ) -> Result<TradeInstruction>;
}
