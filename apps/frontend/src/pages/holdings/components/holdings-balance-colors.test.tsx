import { render, screen } from "@/test/render";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AccountType, AssetKind, HoldingType, QuoteMode } from "@/lib/constants";
import type { Holding } from "@/lib/types";
import { HoldingsGroupedTable } from "./holdings-grouped-table";
import { HoldingsSummaryBar } from "./holdings-summary-bar";

vi.mock("@/hooks/use-balance-privacy", () => ({
  useBalancePrivacy: () => ({ isBalanceHidden: false }),
}));
vi.mock("@/lib/settings-provider", () => ({
  useSettingsContext: () => ({ settings: { baseCurrency: "USD" } }),
}));
vi.mock("react-router-dom", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-router-dom")>()),
  useNavigate: () => vi.fn(),
}));
vi.mock("@/components/ticker-avatar", () => ({ TickerAvatar: () => null }));
const modes = ["green-up", "red-up", "red-up-blue-down"] as const;
afterEach(() => document.documentElement.removeAttribute("data-gain-colors"));

function holding(value: number, liability: boolean, gain = 0): Holding {
  return {
    id: "holding-1",
    accountId: "account-1",
    holdingType: HoldingType.CASH,
    ...(liability ? { assetKind: AssetKind.LIABILITY } : {}),
    instrument: {
      id: "cash:USD",
      symbol: "USD",
      name: "Cash (USD)",
      currency: "USD",
      quoteMode: QuoteMode.MANUAL,
    },
    quantity: 1,
    localCurrency: "USD",
    baseCurrency: "USD",
    marketValue: { local: value, base: value },
    totalGain: { local: gain, base: gain },
    weight: 1,
    asOfDate: "2026-10-30",
  };
}

function table(value: number, liability = true, gain = 0) {
  return (
    <HoldingsGroupedTable
      holdings={[holding(value, liability, gain)]}
      accountTypeMap={
        new Map([["account-1", liability ? AccountType.CREDIT_CARD : AccountType.SECURITIES]])
      }
      linkedLiabilities={new Map()}
      performanceMode="pnl"
      showConvertedValues
      isLoading={false}
    />
  );
}

function amountsIn(root: ParentNode = document.body): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>("span")).filter(
    (element) =>
      element.childElementCount === 0 && /^-?\$[\d,]+\.\d{2}$/.test(element.textContent ?? ""),
  );
}

function amountValue(element: HTMLElement) {
  return Number(element.textContent?.replace(/[$,]/g, "")) || 0;
}

function expectSemantic(element: HTMLElement, value: number, className: string) {
  expect(amountValue(element)).toBe(value === 0 ? 0 : value);
  expect(element).toHaveClass(className);
  expect(element).not.toHaveClass(
    className === "text-success" ? "text-destructive" : "text-success",
  );
  expect(element).not.toHaveClass("text-gain");
  expect(element).not.toHaveClass("text-loss");
}

describe("holdings balance colors are semantic, not gain preferences", () => {
  it.each([100, 0, -100])(
    "keeps net worth %s and debt summary semantics across preferences",
    (netWorth) => {
      const view = render(
        <HoldingsSummaryBar totalAssets={200 + netWorth} totalLiabilities={200} currency="USD" />,
      );
      for (const mode of modes) {
        document.documentElement.setAttribute("data-gain-colors", mode);
        view.rerender(
          <HoldingsSummaryBar totalAssets={200 + netWorth} totalLiabilities={200} currency="USD" />,
        );
        const amounts = amountsIn();
        expect(amountValue(amounts[0])).toBe(200 + netWorth);
        expectSemantic(amounts[1], 200, "text-destructive");
        expectSemantic(amounts[2], netWorth, netWorth >= 0 ? "text-success" : "text-destructive");
      }
    },
  );

  it.each([100, 0, -100])(
    "preserves liability group and row sign models for stored value %s",
    (storedValue) => {
      const view = render(table(storedValue));
      for (const mode of modes) {
        document.documentElement.setAttribute("data-gain-colors", mode);
        view.rerender(table(storedValue));
        const group = amountsIn(screen.getByRole("button", { name: /Liabilities/ }))[0];
        const row = amountsIn().find((amount) => amount !== group)!;
        // Group negates the stored amount; row displays -abs(amount). Keep both contracts.
        expectSemantic(
          group,
          -storedValue,
          -storedValue >= 0 ? "text-success" : "text-destructive",
        );
        expectSemantic(
          row,
          -Math.abs(storedValue),
          storedValue === 0 ? "text-success" : "text-destructive",
        );
        expect(amountsIn()).toHaveLength(2);
      }
    },
  );

  it.each([10, 0, -10])(
    "continues to delegate investment gain %s to AmountDisplay colorFormat",
    (gain) => {
      const view = render(table(100, false, gain));
      for (const mode of modes) {
        document.documentElement.setAttribute("data-gain-colors", mode);
        view.rerender(table(100, false, gain));
        const amounts = amountsIn();
        const gainAmount = amounts.filter(
          (amount) =>
            amount.classList.contains("text-gain") || amount.classList.contains("text-loss"),
        );
        expect(gainAmount).toHaveLength(1);
        expect(amountValue(gainAmount[0])).toBe(gain);
        expect(gainAmount[0]).not.toHaveClass("text-success");
        expect(gainAmount[0]).not.toHaveClass("text-destructive");
        expect(gainAmount[0]).toHaveClass(gain >= 0 ? "text-gain" : "text-loss");
      }
    },
  );
});
