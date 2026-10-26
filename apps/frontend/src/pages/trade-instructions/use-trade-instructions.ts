import { useQuery } from "@tanstack/react-query";
import { getTradeInstruction, listTradeInstructions, type InstructionFilter } from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";
export const instructionKey = [QueryKeys.TRADE_INSTRUCTIONS];
export function useTradeInstructions(filter: InstructionFilter) {
  return useQuery({
    queryKey: [...instructionKey, "list", filter],
    queryFn: () => listTradeInstructions(filter),
    refetchInterval: 15_000,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
  });
}
export function useTradeInstruction(id: string) {
  return useQuery({
    queryKey: [...instructionKey, "detail", id],
    queryFn: () => getTradeInstruction(id),
    refetchInterval: 15_000,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
  });
}
