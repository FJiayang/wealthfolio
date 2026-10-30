import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { searchTicker } from "@/adapters";
import type { SymbolSearchResult } from "@/lib/types";
import { BenchmarkSymbolSelector } from "./benchmark-symbol-selector";
import { BenchmarkSymbolSelectorMobile } from "./benchmark-symbol-selector-mobile";

vi.mock("@/adapters", () => ({ searchTicker: vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

const originalScrollIntoView = Object.getOwnPropertyDescriptor(Element.prototype, "scrollIntoView");
beforeAll(() => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe = vi.fn();
      unobserve = vi.fn();
      disconnect = vi.fn();
    },
  );
  Element.prototype.scrollIntoView = vi.fn();
});
afterAll(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  if (originalScrollIntoView) {
    Object.defineProperty(Element.prototype, "scrollIntoView", originalScrollIntoView);
  } else {
    Reflect.deleteProperty(Element.prototype, "scrollIntoView");
  }
});

describe.each([
  ["desktop", BenchmarkSymbolSelector],
  ["mobile", BenchmarkSymbolSelectorMobile],
] as const)("%s benchmark search", (_name, Selector) => {
  it.each(["ltr", "rtl"])("selects an existing preset security in %s", async (dir) => {
    vi.mocked(searchTicker).mockResolvedValue([
      {
        symbol: "000001.SS",
        longName: "Saved Shanghai Composite",
        existingAssetId: "saved-shanghai",
        score: 100,
      },
      { symbol: "^HSI", longName: "Remote duplicate", score: 90 },
      { symbol: "OTHER", longName: "Other security", score: 80 },
    ] as SymbolSearchResult[]);
    const onSelect = vi.fn();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <div dir={dir}>
          <Selector onSelect={onSelect} />
        </div>
      </QueryClientProvider>,
    );
    fireEvent.click(
      screen.getByRole(_name === "desktop" ? "combobox" : "button", {
        name: "common:component.add_benchmark",
      }),
    );
    fireEvent.change(screen.getByPlaceholderText("common:search_benchmarks"), {
      target: { value: "000001" },
    });
    const saved = await screen.findByText("Saved Shanghai Composite");
    expect(screen.queryByText("Remote duplicate")).not.toBeInTheDocument();
    expect(screen.getByText("Other security")).toBeInTheDocument();
    fireEvent.click(saved);
    await waitFor(() =>
      expect(onSelect).toHaveBeenCalledWith({
        id: "saved-shanghai",
        name: "Saved Shanghai Composite",
        payload: undefined,
      }),
    );
  });
});
