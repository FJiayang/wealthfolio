import type { TradeInstruction } from "@/adapters";

export function instructionAssetLabel(row: TradeInstruction) {
  const from = row.assetSnapshot?.symbol || row.assetSnapshot?.name || row.accountName;
  const to = row.conversion?.toAssetSnapshot;
  return to ? `${from} → ${to.symbol || to.name}` : from;
}
