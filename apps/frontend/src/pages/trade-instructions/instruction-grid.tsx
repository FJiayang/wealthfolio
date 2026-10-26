import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { ColumnDef, OnChangeFn, SortingState } from "@tanstack/react-table";
import {
  Badge,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuCheckboxItem,
  DropdownMenuTrigger,
  Icons,
  Button,
  useNumberFormatting,
  useAmountFormatting,
  useLocalizationSettings,
} from "@wealthfolio/ui";
import { DataGrid, useDataGrid } from "@wealthfolio/ui/components/data-grid";
import type { TradeInstruction } from "@/adapters";
import { useBalancePrivacy } from "@/hooks/use-balance-privacy";
import { ActivityTypeBadge } from "@/pages/activity/components/activity-type-badge";
import { InstructionActions } from "./instruction-actions";
import { ActivityType } from "@/lib/constants";

interface InstructionGridProps {
  items: TradeInstruction[];
  sorting: SortingState;
  onSortingChange: OnChangeFn<SortingState>;
  onOpen: (id: string) => void;
}
export function InstructionGrid({ items, sorting, onSortingChange, onOpen }: InstructionGridProps) {
  const { t } = useTranslation("trade-instructions");
  const { formatQuantity } = useNumberFormatting();
  const { formatAmount } = useAmountFormatting();
  const { isBalanceHidden } = useBalancePrivacy();
  const { locale, timezone } = useLocalizationSettings();
  // Mask the table values too, so copying cells respects balance privacy.
  const data = useMemo(
    () =>
      isBalanceHidden
        ? items.map((row) => ({
            ...row,
            quantity: row.quantity === null ? null : "••••",
            amount: row.amount === null ? null : "••••",
          }))
        : items,
    [items, isBalanceHidden],
  );
  const columns = useMemo<ColumnDef<TradeInstruction>[]>(() => {
    const definitions: ColumnDef<TradeInstruction>[] = [
      {
        accessorKey: "expectedCompletionAt",
        header: t("expectedCompletionDate"),
        size: 220,
        enableSorting: true,
        meta: { cell: { variant: "datetime" } },
      },
      {
        accessorKey: "accountName",
        header: t("account"),
        size: 160,
        meta: { cell: { variant: "select", options: [] } },
      },
      {
        accessorKey: "side",
        header: t("side"),
        size: 110,
        meta: {
          cell: {
            variant: "select",
            options: ["BUY", "SELL"].map((value) => ({ value, label: t(value) })),
            valueRenderer: (value) => (
              <ActivityTypeBadge type={value === "BUY" ? ActivityType.BUY : ActivityType.SELL} />
            ),
          },
        },
      },
      {
        id: "asset",
        header: () => t("asset"),
        meta: { label: t("asset") },
        cell: ({ row }) => (
          <Button
            variant="link"
            className="h-full max-w-full justify-start truncate p-0 text-xs"
            onClick={() => onOpen(row.original.id)}
          >
            {row.original.assetSnapshot.symbol || row.original.assetSnapshot.name}
          </Button>
        ),
        size: 200,
        accessorFn: (row) =>
          [row.assetSnapshot.symbol || row.assetSnapshot.name, row.assetSnapshot.exchangeMic]
            .filter(Boolean)
            .join(" "),
      },
      {
        accessorKey: "quantity",
        header: t("quantity"),
        size: 120,
        meta: {
          renderKey: isBalanceHidden,
          cell: {
            variant: "number",
            valueType: "string",
            valueRenderer: (value) =>
              value === null ? "—" : isBalanceHidden ? "••••" : formatQuantity(value),
          },
        },
      },
      {
        accessorKey: "amount",
        header: t("amount"),
        size: 140,
        meta: {
          renderKey: isBalanceHidden,
          cell: {
            variant: "number",
            valueType: "string",
            valueRenderer: (value, rowData) => {
              const row = rowData as TradeInstruction;
              return value === null
                ? "—"
                : isBalanceHidden
                  ? "••••"
                  : formatAmount(value, row.amountCurrency ?? row.assetSnapshot.quoteCcy, false);
            },
          },
        },
      },
      {
        id: "currency",
        header: t("currency"),
        size: 80,
        accessorFn: (row) => row.amountCurrency ?? row.assetSnapshot.quoteCcy,
      },
      {
        accessorKey: "status",
        header: t("status"),
        size: 160,
        meta: {
          getRenderKey: (row) => row.expectedCompletionAt,
          cell: {
            variant: "select",
            options: ["PENDING", "IN_PROGRESS", "COMPLETED", "CANCELLED"].map((value) => ({
              value,
              label: t(value),
            })),
            valueRenderer: (value, _option, rowData) => {
              const row = rowData as TradeInstruction;
              const overdue =
                (row.status === "PENDING" || row.status === "IN_PROGRESS") &&
                row.expectedCompletionAt &&
                Date.parse(row.expectedCompletionAt) < Date.now();
              return (
                <span className="flex items-center gap-2">
                  <Badge variant="secondary">{t(value)}</Badge>
                  {overdue && <Badge variant="destructive">{t("overdue")}</Badge>}
                </span>
              );
            },
          },
        },
      },
      {
        accessorKey: "completedAt",
        header: t("completedAt"),
        size: 190,
        meta: { cell: { variant: "datetime" } },
      },
      { accessorKey: "notes", header: t("notes"), size: 240 },
      {
        id: "actions",
        header: () => null,
        size: 200,
        minSize: 200,
        maxSize: 200,
        enableResizing: false,
        enableHiding: false,
        enablePinning: false,
        cell: ({ row }) => <InstructionActions row={row.original} />,
      },
    ];
    for (const column of definitions) column.enableSorting ??= false;
    return definitions;
  }, [t, formatQuantity, formatAmount, isBalanceHidden, onOpen]);
  const grid = useDataGrid({
    data,
    columns,
    getRowId: (row) => row.id,
    readOnly: true,
    manualSorting: true,
    enableRowSelection: false,
    enablePinning: false,
    onSortingChange,
    initialState: { sorting, columnPinning: { right: ["actions"] } },
  });
  return (
    <div className="space-y-3">
      <div className="flex justify-end">
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button variant="outline" size="xs" aria-label={t("activity:table_toggle_columns")} />
            }
          >
            <Icons.Settings2 className="h-3.5 w-3.5" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {grid.table
              .getAllLeafColumns()
              .filter((column) => column.getCanHide())
              .map((column) => (
                <DropdownMenuCheckboxItem
                  key={column.id}
                  checked={column.getIsVisible()}
                  onCheckedChange={(visible) => column.toggleVisibility(visible)}
                >
                  {column.columnDef.meta?.label ?? String(column.columnDef.header)}
                </DropdownMenuCheckboxItem>
              ))}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <DataGrid key={`${locale}:${timezone}`} {...grid} stretchColumns height="min(60vh, 640px)" />
    </div>
  );
}
