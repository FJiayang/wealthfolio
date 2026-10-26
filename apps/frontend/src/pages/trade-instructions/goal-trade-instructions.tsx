import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { DateRange } from "react-day-picker";
import type { OnChangeFn, SortingState } from "@tanstack/react-table";
import type { InstructionStatus, TradeInstruction } from "@/adapters";
import { useAccounts } from "@/hooks/use-accounts";
import { usePersistentState } from "@/hooks/use-persistent-state";
import {
  Button,
  FacetedFilter,
  Sheet,
  SheetContent,
  useLocalizationSettings,
} from "@wealthfolio/ui";
import { InstructionPagination } from "./instruction-pagination";
import { DateRangeFilter } from "@/features/spending/components/date-range-filter";
import { useTradeInstructions } from "./use-trade-instructions";
import { InstructionSheet } from "./instruction-sheet";
import { InstructionDetail } from "./instruction-detail";
import { InstructionGrid } from "./instruction-grid";
import { completionDateBounds } from "./time";

const PAGE_SIZES = [20, 50, 100];
const EMPTY_ITEMS: TradeInstruction[] = [];
export function GoalTradeInstructions({ goalId }: { goalId: string }) {
  const { t } = useTranslation("trade-instructions");
  const { timezone = "UTC" } = useLocalizationSettings();
  const [status, setStatus] = useState(new Set(["PENDING", "IN_PROGRESS"]));
  const [account, setAccount] = useState(new Set<string>());
  const [sides, setSides] = useState(new Set<string>());
  const [dateRange, setDateRange] = useState<DateRange>();
  const [storedPageSize, setPageSize] = usePersistentState("trade-instructions-page-size", 20);
  const pageSize = PAGE_SIZES.includes(storedPageSize) ? storedPageSize : 20;
  const [pageIndex, setPageIndex] = useState(0);
  const [sorting, setSorting] = useState<SortingState>([
    { id: "expectedCompletionAt", desc: false },
  ]);
  const [panel, setPanel] = useState<string>();
  const { accounts } = useAccounts({ includeArchived: true, filterActive: false });
  const query = useTradeInstructions({
    goalId,
    statuses: [...status] as InstructionStatus[],
    accountId: [...account][0],
    side: sides.size === 1 ? ([...sides][0] as "BUY" | "SELL") : undefined,
    ...completionDateBounds(dateRange, timezone),
    sort: sorting[0]?.desc ? "expectedCompletionDesc" : "expectedCompletionAsc",
    limit: pageSize,
    offset: pageIndex * pageSize,
  });
  const pageCount = Math.ceil((query.data?.total ?? 0) / pageSize);
  useEffect(() => {
    if (query.data && pageIndex >= pageCount) setPageIndex(Math.max(0, pageCount - 1));
  }, [query.data, pageIndex, pageCount]);
  useEffect(() => {
    setPageIndex(0);
  }, [goalId]);
  const changeSorting: OnChangeFn<SortingState> = (update) => {
    setSorting((previous) => {
      const next = typeof update === "function" ? update(previous) : update;
      return next.length ? next : [{ id: "expectedCompletionAt", desc: false }];
    });
    setPageIndex(0);
  };
  return (
    <div className="space-y-4" data-testid="goal-trade-instructions">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-lg font-semibold">{t("title")}</h2>
        <Button onClick={() => setPanel("new")}>{t("create")}</Button>
      </div>
      <p className="text-muted-foreground text-sm">{t("independent")}</p>
      <div className="flex flex-wrap items-center gap-2" data-testid="instruction-filters">
        <FacetedFilter
          title={t("status")}
          options={["PENDING", "IN_PROGRESS", "COMPLETED", "CANCELLED"].map((value) => ({
            value,
            label: t(value),
          }))}
          selectedValues={status}
          onFilterChange={(next) => {
            setStatus(next);
            setPageIndex(0);
          }}
        />
        <FacetedFilter
          title={t("account")}
          options={accounts.map((item) => ({ value: item.id, label: item.name }))}
          selectedValues={account}
          onFilterChange={(next) => {
            const added = [...next].find((id) => !account.has(id));
            setAccount(new Set(added ? [added] : []));
            setPageIndex(0);
          }}
        />
        <FacetedFilter
          title={t("side")}
          options={["BUY", "SELL"].map((value) => ({ value, label: t(value) }))}
          selectedValues={sides}
          onFilterChange={(next) => {
            setSides(next);
            setPageIndex(0);
          }}
        />
        <DateRangeFilter
          title={t("expectedCompletionDate")}
          value={dateRange}
          onChange={(next) => {
            setDateRange(next);
            setPageIndex(0);
          }}
        />
        <Button
          variant="outline"
          size="sm"
          onClick={() => void query.refetch()}
          disabled={query.isFetching}
        >
          {t("refresh")}
        </Button>
      </div>
      {query.isPending && <p>{t("loading")}</p>}
      {query.isError && (
        <p role="alert" className="text-destructive">
          {String(query.error)}
        </p>
      )}
      {query.data?.items.length === 0 && <p className="text-muted-foreground py-8">{t("empty")}</p>}
      <InstructionGrid
        items={query.data?.items ?? EMPTY_ITEMS}
        sorting={sorting}
        onSortingChange={changeSorting}
        onOpen={setPanel}
      />
      <InstructionPagination
        pageIndex={pageIndex}
        pageSize={pageSize}
        pageCount={pageCount}
        totalRowCount={query.data?.total ?? 0}
        isFetching={query.isFetching}
        onPageChange={setPageIndex}
        onPageSizeChange={(size) => {
          setPageSize(size);
          setPageIndex(0);
        }}
      />
      {panel === "new" && <InstructionSheet goalId={goalId} onClose={() => setPanel(undefined)} />}
      <Sheet
        open={!!panel && panel !== "new"}
        onOpenChange={(open) => {
          if (!open) setPanel(undefined);
        }}
      >
        <SheetContent side="inline-end" className="w-full overflow-y-auto sm:max-w-lg">
          {panel && panel !== "new" ? <InstructionDetail key={panel} id={panel} /> : null}
        </SheetContent>
      </Sheet>
    </div>
  );
}
