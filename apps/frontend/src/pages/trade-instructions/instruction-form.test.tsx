import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { FormattingProvider } from "@wealthfolio/ui";
import { createTradeInstruction } from "@/adapters";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import enInstructions from "@/i18n/locales/en/trade-instructions.json";
import { InstructionForm } from "./instruction-form";

vi.mock("@/adapters", () => ({ createTradeInstruction: vi.fn() }));
vi.mock("@/hooks/use-accounts", () => ({
  useAccounts: () => ({ accounts: [{ id: "account", name: "Investment", currency: "USD" }] }),
}));
vi.mock("@/components/ticker-search", () => ({
  default: ({ onSelectResult }: { onSelectResult: (symbol: string, result: object) => void }) => (
    <button
      type="button"
      onClick={() =>
        onSelectResult("AAPL", { symbol: "AAPL", currency: "USD", quoteType: "EQUITY" })
      }
    >
      Select asset
    </button>
  ),
}));

describe("instruction form financial controls", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe = vi.fn();
        unobserve = vi.fn();
        disconnect = vi.fn();
      },
    );
    HTMLElement.prototype.scrollIntoView = vi.fn();
    vi.mocked(createTradeInstruction).mockReset();
    i18n.addResourceBundle("en", "trade-instructions", enInstructions);
  });
  afterEach(() => vi.unstubAllGlobals());
  it.each(["quantity", "amount"] as const)(
    "submits exact localized %s and price with shared controls",
    async (mode) => {
      const user = userEvent.setup();
      render(
        <FormattingProvider locale="de-DE">
          <QueryClientProvider client={new QueryClient()}>
            <InstructionForm goalId="goal" side="BUY" onSaved={vi.fn()} onCancel={vi.fn()} />
          </QueryClientProvider>
        </FormattingProvider>,
      );
      await user.click(screen.getByRole("button", { name: "Select asset" }));
      await user.click(screen.getByTestId("account-select"));
      await user.click(screen.getByRole("option", { name: /Investment/ }));
      await user.click(
        screen.getByRole("radio", {
          name: mode === "quantity" ? "Quantity" : "Amount",
        }),
      );
      await user.type(
        screen.getByRole("textbox", {
          name: mode === "quantity" ? "Quantity" : "Amount",
        }),
        "1,123456789123456789",
      );
      await user.type(screen.getByRole("textbox", { name: /Limit price/ }), "12,5");
      if (mode === "amount") {
        await user.click(screen.getByRole("combobox", { name: "Currency" }));
        await user.click(screen.getAllByRole("option", { name: /EUR/ })[0]);
      }
      await user.click(screen.getByRole("button", { name: "Add Buy" }));
      await waitFor(() => expect(createTradeInstruction).toHaveBeenCalled());
      expect(vi.mocked(createTradeInstruction).mock.calls[0][0]).toMatchObject({
        goalId: "goal",
        accountId: "account",
        side: "BUY",
        [mode]: "1.123456789123456789",
        limitPrice: "12.5",
        ...(mode === "amount" ? { amountCurrency: "EUR" } : {}),
      });
    },
  );
});
