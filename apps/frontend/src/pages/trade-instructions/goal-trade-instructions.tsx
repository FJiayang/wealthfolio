import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { InstructionStatus, TradeInstruction } from "@/adapters";
import { useBalancePrivacy } from "@/hooks/use-balance-privacy";
import { useAccounts } from "@/hooks/use-accounts";
import { Button } from "@wealthfolio/ui/components/ui/button";
import { Sheet, SheetContent } from "@wealthfolio/ui/components/ui/sheet";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@wealthfolio/ui/components/ui/select";
import { useNumberFormatting, useDateFormatting } from "@wealthfolio/ui";
import { useTradeInstructions } from "./use-trade-instructions";
import { InstructionSheet } from "./instruction-sheet";
import { InstructionDetail } from "./instruction-detail";

function isOverdue(row: TradeInstruction, now = Date.now()) {
  return (
    (row.status === "PENDING" || row.status === "IN_PROGRESS") &&
    !!row.expectedCompletionAt &&
    Date.parse(row.expectedCompletionAt) < now
  );
}
export function GoalTradeInstructions({ goalId }: { goalId: string }) {
  const { t } = useTranslation("trade-instructions");
  const { formatDateTime } = useDateFormatting();
  const { formatQuantity } = useNumberFormatting();
  const { isBalanceHidden } = useBalancePrivacy();
  const [status, setStatus] = useState("open");
  const [account, setAccount] = useState("all");
  const [offset, setOffset] = useState(0);
  const [panel, setPanel] = useState<string>();
  const { accounts } = useAccounts({ includeArchived: true, filterActive: false });
  const statuses: InstructionStatus[] | undefined =
    status === "all" ? [] : status === "open" ? undefined : [status as InstructionStatus];
  const query = useTradeInstructions({
    goalId,
    statuses,
    accountId: account === "all" ? undefined : account,
    limit: 50,
    offset,
  });
  return (
    <div className="space-y-4" data-testid="goal-trade-instructions">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-lg font-semibold">{t("title")}</h2>
        <Button onClick={() => setPanel("new")}>{t("create")}</Button>
      </div>
      <p className="text-muted-foreground text-sm">{t("independent")}</p>
      <div className="flex flex-wrap gap-3">
        <Select
          value={status}
          onValueChange={(value) => {
            setStatus(value ?? "open");
            setOffset(0);
          }}
        >
          <SelectTrigger className="w-44" aria-label={t("status")}>
            <SelectValue>{t(status)}</SelectValue>
          </SelectTrigger>
          <SelectContent>
            {["open", "all", "PENDING", "IN_PROGRESS", "COMPLETED", "CANCELLED"].map((value) => (
              <SelectItem key={value} value={value}>
                {t(value)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select
          value={account}
          onValueChange={(value) => {
            setAccount(value ?? "all");
            setOffset(0);
          }}
        >
          <SelectTrigger className="w-52" aria-label={t("account")}>
            <SelectValue>
              {account === "all"
                ? t("allAccounts")
                : accounts.find((item) => item.id === account)?.name}
            </SelectValue>
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t("allAccounts")}</SelectItem>
            {accounts.map((item) => (
              <SelectItem key={item.id} value={item.id}>
                {item.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button variant="outline" onClick={() => void query.refetch()} disabled={query.isFetching}>
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
      <div className="space-y-2">
        {query.data?.items.map((row) => (
          <button
            key={row.id}
            type="button"
            onClick={() => setPanel(row.id)}
            className="hover:bg-accent focus-visible:ring-ring grid w-full gap-2 rounded-md border p-4 text-start focus-visible:ring-2 sm:grid-cols-3"
          >
            <span>
              <span className="font-medium">
                {t(row.side)} · {row.assetSnapshot.symbol || row.assetSnapshot.name}
              </span>
              <span className="text-muted-foreground block text-sm">
                {row.accountName} · {row.assetSnapshot.exchangeMic}
              </span>
            </span>
            <span>
              {isBalanceHidden
                ? "••••"
                : row.quantity
                  ? `${formatQuantity(row.quantity)} · ${t("quantity")}`
                  : `${formatQuantity(row.amount)} ${row.amountCurrency}`}
              <span className="text-muted-foreground block text-sm">{t(row.status)}</span>
            </span>
            <span className="text-sm">
              {row.completedAt ? (
                <>
                  {t("completedAt")}: {formatDateTime(row.completedAt)}
                </>
              ) : (
                <>
                  {t("expectedCompletionAt")}:{" "}
                  {row.expectedCompletionAt
                    ? formatDateTime(row.expectedCompletionAt)
                    : t("notSet")}
                </>
              )}
              {isOverdue(row) && (
                <span className="text-destructive block font-medium">{t("overdue")}</span>
              )}
            </span>
          </button>
        ))}
      </div>
      <div className="flex justify-end gap-2">
        <Button
          variant="outline"
          disabled={offset === 0}
          onClick={() => setOffset(Math.max(0, offset - 50))}
        >
          {t("previous")}
        </Button>
        <Button
          variant="outline"
          disabled={!query.data || offset + 50 >= query.data.total}
          onClick={() => setOffset(offset + 50)}
        >
          {t("next")}
        </Button>
      </div>
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
