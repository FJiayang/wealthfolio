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
import { INSTRUCTION_SIDES, type InstructionSide, type TradeInstruction } from "@/adapters";
import { useBalancePrivacy } from "@/hooks/use-balance-privacy";
import { InstructionTypeBadge } from "./instruction-display";
import { instructionAssetLabel } from "./instruction-label";
import { InstructionActions } from "./instruction-actions";

interface InstructionGridProps {
  items: TradeInstruction[];
  sorting: SortingState;
  onSortingChange: OnChangeFn<SortingState>;
  onOpen: (id: string) => void;
  onEdit: (id: string) => void;
}
export function InstructionGrid({
  items,
  sorting,
  onSortingChange,
  onOpen,
  onEdit,
}: InstructionGridProps) {
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
            transfer: row.transfer
              ? {
                  ...row.transfer,
                  destinationAmount: row.transfer.destinationAmount ? "••••" : null,
                  unitPrice: row.transfer.unitPrice ? "••••" : null,
                }
              : row.transfer,
            conversion: row.conversion
              ? { ...row.conversion, toQuantity: row.conversion.toQuantity ? "••••" : null }
              : row.conversion,
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
        meta: {
          cell: {
            variant: "select",
            options: [],
            valueRenderer: (_value, _option, data) => {
              const row = data as TradeInstruction;
              return row.transfer ? (
                <span className="truncate">
                  {row.transfer.isExternal
                    ? row.transfer.direction === "in"
                      ? `${t("external")} → ${row.accountName}`
                      : `${row.accountName} → ${t("external")}`
                    : `${row.accountName} → ${row.transfer.toAccountName}`}
                </span>
              ) : (
                row.accountName
              );
            },
          },
        },
      },
      {
        accessorKey: "side",
        header: t("side"),
        size: 110,
        meta: {
          cell: {
            variant: "select",
            options: INSTRUCTION_SIDES.map((value) => ({ value, label: t(value) })),
            valueRenderer: (value) => <InstructionTypeBadge side={value as InstructionSide} />,
          },
        },
      },
      {
        id: "asset",
        header: () => t("asset"),
        meta: { label: t("asset") },
        cell: ({ row }) => (
          <div className="flex h-full min-w-0 items-center gap-2">
            <Button
              variant="link"
              className="h-full max-w-full justify-start truncate p-0 text-xs"
              onClick={() => onOpen(row.original.id)}
            >
              {instructionAssetLabel(row.original)}
            </Button>
            {row.original.assetSnapshot?.symbol &&
              row.original.assetSnapshot?.name &&
              row.original.assetSnapshot?.name !== row.original.assetSnapshot?.symbol && (
                <Badge
                  variant="secondary"
                  className="min-w-0 truncate"
                  title={row.original.assetSnapshot?.name}
                >
                  {row.original.assetSnapshot?.name}
                </Badge>
              )}
          </div>
        ),
        size: 320,
        accessorFn: (row) =>
          [instructionAssetLabel(row), row.assetSnapshot?.exchangeMic].filter(Boolean).join(" "),
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
                  : formatAmount(
                      value,
                      row.amountCurrency ?? row.assetSnapshot?.quoteCcy ?? "",
                      false,
                    );
            },
          },
        },
      },
      {
        id: "currency",
        header: t("currency"),
        size: 80,
        accessorFn: (row) => row.amountCurrency ?? row.assetSnapshot?.quoteCcy ?? "",
      },
      {
        id: "destination",
        header: t("destination"),
        size: 190,
        accessorFn: (row) => {
          if (row.conversion)
            return row.conversion.toQuantity == null
              ? t("undetermined")
              : isBalanceHidden
                ? "••••"
                : formatQuantity(row.conversion.toQuantity);
          if (row.transfer?.mode === "cash" && !row.transfer.isExternal)
            return row.transfer.destinationAmount == null
              ? t("undetermined")
              : isBalanceHidden
                ? "••••"
                : formatAmount(
                    row.transfer.destinationAmount,
                    row.transfer.destinationCurrency ?? "",
                    false,
                  );
          return "—";
        },
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
        header: () => t("actions"),
        size: 240,
        minSize: 240,
        maxSize: 240,
        enableResizing: false,
        enableHiding: false,
        enablePinning: false,
        cell: ({ row }) => (
          <InstructionActions row={row.original} onOpen={onOpen} onEdit={onEdit} />
        ),
      },
    ];
    for (const column of definitions) column.enableSorting ??= false;
    return definitions;
  }, [t, formatQuantity, formatAmount, isBalanceHidden, onOpen, onEdit]);
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
