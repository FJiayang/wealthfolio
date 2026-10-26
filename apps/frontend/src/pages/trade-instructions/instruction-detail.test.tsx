import { render, screen, waitFor, within } from "@/test/render";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { FormattingProvider, Sheet, SheetContent } from "@wealthfolio/ui";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import enActivity from "@/i18n/locales/en/activity.json";
import {
  deleteTradeInstruction,
  startTradeInstruction,
  completeTradeInstruction,
  cancelTradeInstruction,
  type InstructionFilter,
  type TradeInstruction,
} from "@/adapters";
import enInstructions from "@/i18n/locales/en/trade-instructions.json";
import { GoalTradeInstructions } from "./goal-trade-instructions";
import { InstructionDetail } from "./instruction-detail";
import { InstructionGrid } from "./instruction-grid";

// jsdom has no viewport; render the real grid with all rows visible.
vi.mock("@wealthfolio/ui/components/data-grid", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@wealthfolio/ui/components/data-grid")>();
  return {
    ...actual,
    DataGrid: (props: React.ComponentProps<typeof actual.DataGrid<TradeInstruction>>) => (
      <actual.DataGrid
        {...props}
        virtualTotalSize={props.table.getRowModel().rows.length * 36}
        virtualItems={props.table.getRowModel().rows.map((row, index) => ({
          key: row.id,
          index,
          start: index * 36,
          end: (index + 1) * 36,
          size: 36,
          lane: 0,
        }))}
      />
    ),
  };
});
vi.mock("@/adapters", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/adapters")>()),
  deleteTradeInstruction: vi.fn(),
  startTradeInstruction: vi.fn(),
  completeTradeInstruction: vi.fn(),
  cancelTradeInstruction: vi.fn(),
}));
function renderList() {
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  return render(<GoalTradeInstructions goalId="goal" />, {
    wrapper: ({ children }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  });
}
const state = vi.hoisted(() => ({
  hidden: false,
  total: 2,
  filter: undefined as InstructionFilter | undefined,
}));
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
  useTradeInstructions: (filter: InstructionFilter) => {
    state.filter = filter;
    return {
      data: {
        items: [
          instruction,
          {
            ...instruction,
            id: "amount",
            quantity: null,
            amount: "2345.67",
            amountCurrency: "USD",
          },
        ],
        total: state.total,
      },
      isPending: false,
      isError: false,
    };
  },
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
      const { rerender } = renderList();
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

describe("instruction pagination and filters", () => {
  beforeEach(() => {
    document.elementsFromPoint = vi.fn(() => []);
    HTMLElement.prototype.scrollIntoView = vi.fn();
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe = vi.fn();
        unobserve = vi.fn();
        disconnect = vi.fn();
      },
    );
  });
  beforeEach(() => {
    localStorage.clear();
    state.total = 61;
    state.hidden = false;
    i18n.addResourceBundle("en", "trade-instructions", enInstructions);
    i18n.addResourceBundle("en", "activity", enActivity);
  });
  it("defaults to 20, changes pages, persists size, and restores it on remount", async () => {
    const user = userEvent.setup();
    const view = renderList();
    expect(state.filter).toMatchObject({
      limit: 20,
      offset: 0,
      sort: "expectedCompletionAsc",
      statuses: ["PENDING", "IN_PROGRESS"],
    });
    await user.click(screen.getByRole("button", { name: "Next page" }));
    expect(state.filter?.offset).toBe(20);
    await user.click(screen.getByRole("combobox", { name: "Rows per page" }));
    await user.click(await screen.findByRole("option", { name: "50" }));
    expect(state.filter).toMatchObject({ limit: 50, offset: 0 });
    expect(localStorage.getItem("trade-instructions-page-size")).toBe("50");
    view.unmount();
    renderList();
    expect(state.filter).toMatchObject({ limit: 50, offset: 0 });
  });
  it("resets the page on filtering and clamps it after totals shrink", async () => {
    const user = userEvent.setup();
    const view = renderList();
    await user.click(screen.getByRole("button", { name: "Next page" }));
    await user.click(
      within(screen.getByTestId("instruction-filters")).getByRole("button", { name: "Side" }),
    );
    await user.click(screen.getByRole("option", { name: "Sell" }));
    await user.keyboard("{Escape}");
    expect(state.filter).toMatchObject({ side: "SELL", offset: 0 });
    await user.click(screen.getByRole("button", { name: "Go to last page" }));
    expect(state.filter?.offset).toBe(60);
    state.total = 21;
    view.rerender(<GoalTradeInstructions goalId="goal" />);
    await waitFor(() => expect(state.filter?.offset).toBe(20));
  });
});

describe("instruction quick actions", () => {
  beforeEach(() => {
    localStorage.clear();
    state.hidden = false;
    state.total = 2;
    vi.clearAllMocks();
    document.elementsFromPoint = vi.fn(() => []);
    HTMLElement.prototype.scrollIntoView = vi.fn();
    i18n.addResourceBundle("en", "trade-instructions", enInstructions);
    i18n.addResourceBundle("en", "activity", enActivity);
  });
  it("starts directly from the frozen actions column", async () => {
    const user = userEvent.setup();
    vi.mocked(startTradeInstruction).mockResolvedValue({
      ...instruction,
      status: "IN_PROGRESS",
      version: 2,
    });
    renderList();
    const actionCells = screen
      .getAllByRole("gridcell")
      .filter((cell) => cell.dataset.columnId === "actions");
    expect(actionCells).toHaveLength(2);
    expect(actionCells[0]).toHaveStyle({ position: "sticky" });
    await user.click(within(actionCells[0]).getByRole("button", { name: "Start" }));
    await waitFor(() => expect(startTradeInstruction).toHaveBeenCalledWith("instruction", 1));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Toggle columns" }));
    expect(await screen.findAllByRole("menuitemcheckbox")).toHaveLength(10);
    expect(screen.queryByRole("menuitemcheckbox", { name: /actions/i })).not.toBeInTheDocument();
  });
  it.each(["Complete", "Cancel"])(
    "%s asks for a resolution without opening details",
    async (label) => {
      const user = userEvent.setup();
      vi.mocked(completeTradeInstruction).mockResolvedValue({
        ...instruction,
        status: "COMPLETED",
        version: 2,
      });
      vi.mocked(cancelTradeInstruction).mockResolvedValue({
        ...instruction,
        status: "CANCELLED",
        version: 2,
      });
      renderList();
      const cells = screen
        .getAllByRole("gridcell")
        .filter((cell) => cell.dataset.columnId === "actions");
      await user.click(within(cells[0]).getByRole("button", { name: label }));
      const dialog = screen.getByRole("dialog");
      expect(within(dialog).queryByText("Financial Details")).not.toBeInTheDocument();
      const noteInput = within(dialog).getByLabelText(
        label === "Complete" ? "Resolution note (optional)" : "Resolution note",
      );
      if (label === "Complete") expect(noteInput).not.toBeRequired();
      else expect(noteInput).toBeRequired();
      await user.type(noteInput, "Handled externally");
      await user.click(within(dialog).getByRole("button", { name: "Confirm" }));
      await waitFor(() =>
        expect(
          label === "Complete" ? completeTradeInstruction : cancelTradeInstruction,
        ).toHaveBeenCalled(),
      );
      if (label === "Complete")
        expect(completeTradeInstruction).toHaveBeenCalledWith(
          "instruction",
          expect.objectContaining({
            expectedVersion: 1,
            resolutionNote: "Handled externally",
          }),
        );
      else
        expect(cancelTradeInstruction).toHaveBeenCalledWith("instruction", 1, "Handled externally");
    },
  );
});

describe("instruction completion defaults and row menu", () => {
  beforeEach(() => {
    localStorage.clear();
    state.hidden = false;
    state.total = 2;
    vi.clearAllMocks();
    document.elementsFromPoint = vi.fn(() => []);
    HTMLElement.prototype.scrollIntoView = vi.fn();
    i18n.addResourceBundle("en", "trade-instructions", enInstructions);
    i18n.addResourceBundle("en", "activity", enActivity);
  });
  afterEach(() => vi.useRealTimers());
  it("defaults the quick completion to now and refreshes only after reopening", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-10-26T08:42:59Z"));
    const user = userEvent.setup();
    vi.mocked(completeTradeInstruction).mockResolvedValue({
      ...instruction,
      status: "COMPLETED",
      version: 2,
    });
    const view = renderList();
    await user.click(screen.getAllByRole("button", { name: "Complete" })[0]);
    let dialog = screen.getByRole("dialog");
    expect(
      within(dialog).getByText(
        "Defaults to the current time. Clear to use the server time when confirming.",
      ),
    ).toBeVisible();
    vi.setSystemTime(new Date("2026-10-26T08:45:00Z"));
    view.rerender(<GoalTradeInstructions goalId="goal" />);
    await user.click(within(dialog).getByRole("button", { name: "Confirm" }));
    await waitFor(() =>
      expect(completeTradeInstruction).toHaveBeenCalledWith("instruction", {
        expectedVersion: 1,
        resolutionNote: "",
        completedAt: "2026-10-26T08:42:00.000Z",
      }),
    );
    await user.click(screen.getAllByRole("button", { name: "Complete" })[0]);
    dialog = screen.getByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Confirm" }));
    await waitFor(() =>
      expect(completeTradeInstruction).toHaveBeenLastCalledWith("instruction", {
        expectedVersion: 1,
        resolutionNote: "",
        completedAt: "2026-10-26T08:45:00.000Z",
      }),
    );
  });
  it("uses the application timezone and minute precision in the detail completion form", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-10-26T08:42:59Z"));
    const user = userEvent.setup();
    vi.mocked(completeTradeInstruction).mockResolvedValue({
      ...instruction,
      status: "COMPLETED",
      version: 2,
    });
    render(
      <FormattingProvider locale="en-US" timezone="Asia/Shanghai">
        <QueryClientProvider client={new QueryClient()}>
          <Sheet open>
            <SheetContent>
              <InstructionDetail id="instruction" />
            </SheetContent>
          </Sheet>
        </QueryClientProvider>
      </FormattingProvider>,
    );
    await user.click(await screen.findByRole("button", { name: "Mark complete" }));
    expect(screen.getByLabelText("Resolution note (optional)")).not.toBeRequired();
    expect(
      screen.getByTestId("instruction-completed-picker").querySelector('[data-type="hour"]'),
    ).toHaveTextContent("4");
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() =>
      expect(completeTradeInstruction).toHaveBeenCalledWith("instruction", {
        expectedVersion: 1,
        resolutionNote: "",
        completedAt: "2026-10-26T08:42:00.000Z",
      }),
    );
  });
  it("shows the action heading, name badge and detail/edit/delete menu without duplicate", async () => {
    const user = userEvent.setup();
    renderList();
    expect(screen.getByRole("columnheader", { name: "Actions" })).toBeVisible();
    expect(screen.getAllByText("Apple")[0]).toHaveAttribute("data-slot", "badge");
    await user.click(screen.getAllByRole("button", { name: "Open menu" })[0]);
    expect(await screen.findAllByRole("menuitem")).toHaveLength(3);
    expect(screen.getByRole("menuitem", { name: "Edit instruction" })).toBeEnabled();
    expect(screen.queryByRole("menuitem", { name: /duplicate/i })).not.toBeInTheDocument();
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    const confirm = screen.getByRole("alertdialog");
    expect(
      within(confirm).getByText(/Permanently delete AAPL in Investment account/),
    ).toBeVisible();
    await user.click(within(confirm).getByRole("button", { name: "Back" }));
    expect(deleteTradeInstruction).not.toHaveBeenCalled();
    await user.click(screen.getAllByRole("button", { name: "Open menu" })[0]);
    await user.click(await screen.findByRole("menuitem", { name: "Delete" }));
    vi.mocked(deleteTradeInstruction).mockResolvedValue();
    await user.click(
      within(screen.getByRole("alertdialog")).getByRole("button", { name: "Delete" }),
    );
    await waitFor(() => expect(deleteTradeInstruction).toHaveBeenCalledWith("instruction", 1));
  });
});

it.each([
  { symbol: null, name: "Apple", display: "Apple" },
  { symbol: "AAPL", name: "AAPL", display: "AAPL" },
  { symbol: "AAPL", name: null, display: "AAPL" },
])("does not duplicate the asset label for $symbol / $name", ({ symbol, name, display }) => {
  state.hidden = false;
  render(
    <QueryClientProvider client={new QueryClient()}>
      <InstructionGrid
        items={[{ ...instruction, assetSnapshot: { ...instruction.assetSnapshot, symbol, name } }]}
        sorting={[]}
        onSortingChange={vi.fn()}
        onOpen={vi.fn()}
        onEdit={vi.fn()}
      />
    </QueryClientProvider>,
  );
  expect(screen.getAllByText(display)).toHaveLength(1);
});
