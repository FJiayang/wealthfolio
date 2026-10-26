import { render, screen } from "@/test/render";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Sheet, SheetContent } from "@wealthfolio/ui";
import i18n from "i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TradeInstruction } from "@/adapters";
import enInstructions from "@/i18n/locales/en/trade-instructions.json";
import { GoalTradeInstructions } from "./goal-trade-instructions";
import { InstructionDetail } from "./instruction-detail";

const state = vi.hoisted(() => ({ hidden: false }));
vi.mock("@/hooks/use-accounts", () => ({ useAccounts: () => ({ accounts: [] }) }));
vi.mock("@/hooks/use-balance-privacy", () => ({
  useBalancePrivacy: () => ({ isBalanceHidden: state.hidden }),
}));
const instruction: TradeInstruction = {
  id: "instruction",
  requestId: "request",
  goalId: "goal",
  accountId: "account",
  accountName: "Investment account",
  assetId: "asset",
  assetSnapshot: {
    name: "Apple",
    symbol: "AAPL",
    exchangeMic: "XNAS",
    instrumentType: "EQUITY",
    quoteCcy: "USD",
  },
  side: "BUY",
  quantity: "1000",
  amount: null,
  amountCurrency: null,
  limitPrice: "12.34",
  notes: "Optional execution note",
  expectedCompletionAt: "2026-10-26T00:00:00Z",
  status: "PENDING",
  resolutionNote: null,
  source: "UI",
  createdAt: "2026-10-25T00:00:00Z",
  updatedAt: "2026-10-25T01:00:00Z",
  startedAt: null,
  closedAt: null,
  completedAt: null,
  version: 1,
};
vi.mock("./use-trade-instructions", () => ({
  instructionKey: ["trade-instructions"],
  useTradeInstructions: () => ({
    data: {
      items: [
        instruction,
        { ...instruction, id: "amount", quantity: null, amount: "2345.67", amountCurrency: "USD" },
      ],
      total: 2,
    },
    isPending: false,
    isError: false,
  }),
  useTradeInstruction: () => ({ data: instruction, isPending: false, isError: false }),
}));
function showDetail() {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <Sheet open>
        <SheetContent>
          <InstructionDetail id="instruction" />
        </SheetContent>
      </Sheet>
    </QueryClientProvider>,
  );
}
describe("instruction detail", () => {
  beforeEach(() => {
    state.hidden = false;
    i18n.addResourceBundle("en", "trade-instructions", enInstructions);
  });
  it("groups the snapshot, financial values, notes and record timestamps", async () => {
    showDetail();
    expect(await screen.findByRole("heading", { name: "Transaction" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Financial Details" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Record Info" })).toBeVisible();
    expect(screen.getByText("Updated")).toBeVisible();
    expect(screen.getByText("Optional execution note")).toBeVisible();
    expect(screen.getAllByText("1,000")).toHaveLength(2);
    expect(screen.getByText("$12.34")).toBeVisible();
    expect(screen.getByRole("button", { name: "Mark complete" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Cancel instruction" })).toBeVisible();
  });
  it("hides the summary and financial values with the system privacy setting", async () => {
    state.hidden = true;
    showDetail();
    expect(await screen.findAllByText("••••")).toHaveLength(3);
    expect(screen.queryByText("1,000")).not.toBeInTheDocument();
    expect(screen.queryByText("$12.34")).not.toBeInTheDocument();
    expect(screen.getByText("Optional execution note")).toBeVisible();
  });
});

describe("instruction list privacy", () => {
  it.each([false, true])(
    "applies balance privacy to quantities and amounts (hidden: %s)",
    (hidden) => {
      state.hidden = hidden;
      i18n.addResourceBundle("en", "trade-instructions", enInstructions);
      const { rerender } = render(<GoalTradeInstructions goalId="goal" />);
      if (hidden) {
        expect(screen.getAllByText("••••")).toHaveLength(2);
        expect(screen.queryByText(/1,000/)).not.toBeInTheDocument();
        expect(screen.queryByText(/2,345.67/)).not.toBeInTheDocument();
      } else {
        expect(screen.getByText(/1,000/)).toBeVisible();
        expect(screen.getByText(/2,345.67/)).toBeVisible();
      }
      state.hidden = !hidden;
      rerender(<GoalTradeInstructions goalId="goal" />);
      expect(screen.queryAllByText("••••")).toHaveLength(hidden ? 0 : 2);
    },
  );
});
