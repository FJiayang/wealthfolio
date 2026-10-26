import { invoke } from "./platform";
import type { AssetResolutionInput } from "@/lib/types";

export type InstructionStatus = "PENDING" | "IN_PROGRESS" | "COMPLETED" | "CANCELLED";
export interface NewTradeInstruction {
  goalId: string;
  requestId: string;
  accountId: string;
  asset: AssetResolutionInput;
  side: "BUY" | "SELL";
  quantity?: string;
  amount?: string;
  amountCurrency?: string;
  limitPrice?: string;
  notes?: string;
  expectedCompletionAt?: string;
}
export interface TradeInstruction {
  goalId: string;
  id: string;
  requestId: string;
  accountId: string;
  accountName: string;
  assetId: string;
  assetSnapshot: {
    name: string | null;
    symbol: string | null;
    exchangeMic: string | null;
    instrumentType: string | null;
    quoteCcy: string;
  };
  side: "BUY" | "SELL";
  quantity: string | null;
  amount: string | null;
  amountCurrency: string | null;
  limitPrice: string | null;
  notes: string | null;
  expectedCompletionAt: string | null;
  status: InstructionStatus;
  resolutionNote: string | null;
  source: "UI" | "MCP";
  createdAt: string;
  updatedAt: string;
  startedAt: string | null;
  closedAt: string | null;
  completedAt: string | null;
  version: number;
}
export interface InstructionFilter {
  goalId?: string;
  statuses?: InstructionStatus[];
  accountId?: string;
  side?: "BUY" | "SELL";
  expectedCompletionFrom?: string;
  expectedCompletionBefore?: string;
  sort?: "expectedCompletionAsc" | "expectedCompletionDesc";
  limit?: number;
  offset?: number;
}
export interface InstructionPage {
  items: TradeInstruction[];
  total: number;
}
export interface CompleteInstruction {
  expectedVersion: number;
  resolutionNote: string;
  completedAt?: string;
}
export const createTradeInstruction = (input: NewTradeInstruction) =>
  invoke<TradeInstruction>("create_trade_instruction", { input });
export const listTradeInstructions = (filter: InstructionFilter) =>
  invoke<InstructionPage>("list_trade_instructions", { filter });
export const getTradeInstruction = (id: string) =>
  invoke<TradeInstruction>("get_trade_instruction", { id });
export const startTradeInstruction = (id: string, expectedVersion: number) =>
  invoke<TradeInstruction>("start_trade_instruction", { id, input: { expectedVersion } });
export const completeTradeInstruction = (id: string, input: CompleteInstruction) =>
  invoke<TradeInstruction>("complete_trade_instruction", { id, input });
export const cancelTradeInstruction = (
  id: string,
  expectedVersion: number,
  resolutionNote: string,
) =>
  invoke<TradeInstruction>("cancel_trade_instruction", {
    id,
    input: { expectedVersion, resolutionNote },
  });
export const correctTradeInstructionCompletionTime = (
  id: string,
  expectedVersion: number,
  completedAt: string,
) =>
  invoke<TradeInstruction>("correct_trade_instruction_completion_time", {
    id,
    input: { expectedVersion, completedAt },
  });
