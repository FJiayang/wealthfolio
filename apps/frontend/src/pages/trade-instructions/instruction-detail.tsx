import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  cancelTradeInstruction,
  completeTradeInstruction,
  correctTradeInstructionCompletionTime,
  startTradeInstruction,
  type TradeInstruction,
} from "@/adapters";
import { Button } from "@wealthfolio/ui/components/ui/button";
import {
  Badge,
  DatePickerInput,
  Icons,
  Separator,
  SheetHeader,
  SheetTitle,
  useAmountFormatting,
} from "@wealthfolio/ui";
import { useBalancePrivacy } from "@/hooks/use-balance-privacy";
import { DetailRow, DetailSection } from "@/pages/activity/components/activity-detail-sheet";
import { ActivityTypeBadge } from "@/pages/activity/components/activity-type-badge";
import { Label } from "@wealthfolio/ui/components/ui/label";
import { Textarea } from "@wealthfolio/ui/components/ui/textarea";
import { useNumberFormatting, useDateFormatting, useLocalizationSettings } from "@wealthfolio/ui";
import { inputTime, instantTime, pickerTime } from "./time";
import { instructionKey, useTradeInstruction } from "./use-trade-instructions";

const COMPLETE_ACTION_CLASS = "bg-success bg-none text-success-foreground hover:bg-success/90";
const CANCEL_ACTION_CLASS =
  "bg-destructive bg-none text-destructive-foreground hover:bg-destructive/90 dark:bg-destructive/60 dark:hover:bg-destructive/70";

type Action = "start" | "complete" | "cancel" | "correct";
export function InstructionDetail({ id }: { id: string }) {
  const { t } = useTranslation("trade-instructions");
  const { timezone = "UTC" } = useLocalizationSettings();
  const { formatDateTime, formatDate } = useDateFormatting();
  const { formatAmount, formatPrice } = useAmountFormatting();
  const { isBalanceHidden } = useBalancePrivacy();
  const { formatQuantity } = useNumberFormatting();
  const queryClient = useQueryClient();
  const query = useTradeInstruction(id);
  const [action, setAction] = useState<{ kind: Action; row: TradeInstruction }>();
  const [note, setNote] = useState("");
  const [time, setTime] = useState("");
  const mutation = useMutation({
    mutationFn: async () => {
      if (!action) throw new Error("No action selected");
      const { row, kind } = action;
      if (kind === "start") return startTradeInstruction(row.id, row.version);
      if (kind === "cancel") return cancelTradeInstruction(row.id, row.version, note);
      if (kind === "correct")
        return correctTradeInstructionCompletionTime(
          row.id,
          row.version,
          instantTime(time, timezone),
        );
      return completeTradeInstruction(row.id, {
        expectedVersion: row.version,
        resolutionNote: note,
        ...(time ? { completedAt: instantTime(time, timezone) } : {}),
      });
    },
    onSuccess: async () => {
      setAction(undefined);
      await queryClient.invalidateQueries({ queryKey: instructionKey });
    },
    onError: async () => {
      setAction(undefined);
      await queryClient.invalidateQueries({ queryKey: instructionKey });
    },
  });
  const header = (
    <SheetHeader className="pb-4">
      <SheetTitle className="flex items-center gap-3">
        <div className="bg-primary/10 flex h-10 w-10 items-center justify-center rounded-full">
          <Icons.Receipt className="text-primary h-5 w-5" />
        </div>
        <div className="flex flex-col items-start">
          <span>{t("detail")}</span>
          {query.data && (
            <span className="text-muted-foreground text-xs font-normal">
              {query.data.assetSnapshot.symbol || query.data.assetSnapshot.name}
            </span>
          )}
        </div>
      </SheetTitle>
    </SheetHeader>
  );
  if (query.isPending)
    return (
      <>
        {header}
        <p>{t("loading")}</p>
      </>
    );
  if (query.isError)
    return (
      <>
        {header}
        <p role="alert">{String(query.error)}</p>
      </>
    );
  const row = query.data;
  const begin = (kind: Action) => {
    mutation.reset();
    setNote("");
    setTime(
      kind === "correct" && row.completedAt
        ? inputTime(row.completedAt, timezone)
        : kind === "complete"
          ? inputTime(new Date().toISOString(), timezone)
          : "",
    );
    setAction({ kind, row });
  };
  const date = (value: string | null) => (value ? formatDateTime(value) : t("notSet"));
  const sizeLabel = t(row.quantity !== null ? "quantity" : "amount");
  const size = isBalanceHidden
    ? "••••"
    : row.quantity !== null
      ? formatQuantity(row.quantity)
      : formatAmount(row.amount, row.amountCurrency!);
  const price =
    row.limitPrice === null
      ? t("notSet")
      : isBalanceHidden
        ? "••••"
        : formatPrice(row.limitPrice, row.assetSnapshot.quoteCcy);
  const late =
    row.expectedCompletionAt &&
    row.completedAt &&
    Date.parse(row.completedAt) > Date.parse(row.expectedCompletionAt);
  return (
    <>
      {header}
      <div className="space-y-6 pb-6 md:pb-8">
        <div className="from-primary/5 to-primary/10 rounded-xl border bg-gradient-to-br p-4">
          <div className="flex items-start justify-between gap-4">
            <div className="min-w-0">
              <div className="text-muted-foreground mb-1 text-xs tracking-wide uppercase">
                {t(row.side)}
              </div>
              <div className="text-xl font-bold break-words">{row.assetSnapshot.symbol}</div>
              <div className="text-muted-foreground text-sm break-words">
                {row.assetSnapshot.name}
              </div>
            </div>
            <Badge
              variant={
                row.status === "CANCELLED"
                  ? "destructive"
                  : row.status === "COMPLETED"
                    ? "default"
                    : "secondary"
              }
            >
              {t(row.status)}
            </Badge>
          </div>
          <Separator className="my-3" />
          <div className="grid grid-cols-2 gap-4">
            <div>
              <div className="text-muted-foreground text-xs">{t("expectedCompletionAt")}</div>
              <div className="font-medium">
                {row.expectedCompletionAt ? formatDate(row.expectedCompletionAt) : t("notSet")}
              </div>
            </div>
            <div className="text-end">
              <div className="text-muted-foreground text-xs">{sizeLabel}</div>
              <div className="text-lg font-bold">{size}</div>
            </div>
          </div>
        </div>
        <DetailSection
          title={t("activity:detail.transaction")}
          icon={<Icons.ArrowLeftRight className="h-4 w-4" />}
        >
          <DetailRow label={t("side")} value={<ActivityTypeBadge type={row.side} />} />
          <DetailRow label={t("account")} value={row.accountName} />
          <DetailRow
            label={t("asset")}
            value={[row.assetSnapshot.symbol, row.assetSnapshot.exchangeMic]
              .filter(Boolean)
              .join(" · ")}
          />
          <DetailRow label={t("expectedCompletionAt")} value={date(row.expectedCompletionAt)} />
          <DetailRow label={t("completedAt")} value={date(row.completedAt)} />
          {row.completedAt && row.expectedCompletionAt && (
            <p className="text-muted-foreground pt-2 text-xs">
              {t(late ? "completedLate" : "completedOnTime")}
            </p>
          )}
        </DetailSection>
        <DetailSection
          title={t("activity:detail.financial_details")}
          icon={<Icons.DollarSign className="h-4 w-4" />}
        >
          <DetailRow label={sizeLabel} value={size} />
          <DetailRow label={t("limitPrice")} value={price} />
          <DetailRow
            label={t("currency")}
            value={row.amountCurrency ?? row.assetSnapshot.quoteCcy}
          />
        </DetailSection>
        {row.notes && (
          <DetailSection
            title={t("activity:detail.notes")}
            icon={<Icons.FileText className="h-4 w-4" />}
          >
            <p className="text-sm break-words whitespace-pre-wrap">{row.notes}</p>
          </DetailSection>
        )}
        {row.resolutionNote && (
          <DetailSection title={t("resolutionNote")} icon={<Icons.FileText className="h-4 w-4" />}>
            <p className="text-sm break-words whitespace-pre-wrap">{row.resolutionNote}</p>
          </DetailSection>
        )}
        <DetailSection
          title={t("activity:detail.record_info")}
          icon={<Icons.Info className="h-4 w-4" />}
        >
          <DetailRow label={t("activity:detail.created")} value={date(row.createdAt)} />
          <DetailRow label={t("activity:detail.updated")} value={date(row.updatedAt)} />
          <DetailRow label={t("closedAt")} value={date(row.closedAt)} />
          <DetailRow label={t("source")} value={row.source} />
        </DetailSection>
        {!action && (
          <div className="flex flex-wrap gap-2">
            {row.status === "PENDING" && (
              <Button disabled={mutation.isPending} onClick={() => begin("start")}>
                {t("start")}
              </Button>
            )}
            {(row.status === "PENDING" || row.status === "IN_PROGRESS") && (
              <>
                <Button
                  className={COMPLETE_ACTION_CLASS}
                  disabled={mutation.isPending}
                  onClick={() => begin("complete")}
                >
                  {t("complete")}
                </Button>
                <Button
                  className={CANCEL_ACTION_CLASS}
                  disabled={mutation.isPending}
                  onClick={() => begin("cancel")}
                >
                  {t("cancel")}
                </Button>
              </>
            )}
            {row.status === "COMPLETED" && (
              <Button variant="outline" onClick={() => begin("correct")}>
                {t("correct")}
              </Button>
            )}
          </div>
        )}
        {action && (
          <form
            className="bg-muted/30 space-y-3 rounded-lg border p-3"
            onSubmit={(event) => {
              event.preventDefault();
              mutation.mutate();
            }}
          >
            <p className="font-medium">{t(action.kind)}</p>
            {(action.kind === "complete" || action.kind === "cancel") && (
              <div>
                <Label htmlFor="resolution-note">
                  {t(action.kind === "complete" ? "completionNote" : "resolutionNote")}
                </Label>
                <Textarea
                  id="resolution-note"
                  required={action.kind === "cancel"}
                  maxLength={4000}
                  value={note}
                  onChange={(event) => setNote(event.target.value)}
                  disabled={mutation.isPending}
                />
              </div>
            )}
            {(action.kind === "complete" || action.kind === "correct") && (
              <div>
                <Label htmlFor="completed-time">
                  {t("completedAt")} ({timezone})
                </Label>
                <DatePickerInput
                  id="completed-time"
                  aria-label={`${t("completedAt")} (${timezone})`}
                  data-testid="instruction-completed-picker"
                  enableTime
                  timeZone={timezone}
                  timeGranularity="minute"
                  isRequired={action.kind === "correct"}
                  value={time || undefined}
                  onChange={(date) => setTime(pickerTime(date, timezone))}
                  disabled={mutation.isPending}
                />
                {action.kind === "complete" && (
                  <p className="text-muted-foreground text-xs">{t("completionDefault")}</p>
                )}
              </div>
            )}
            <div className="flex gap-2">
              <Button
                type="submit"
                className={
                  action.kind === "complete"
                    ? COMPLETE_ACTION_CLASS
                    : action.kind === "cancel"
                      ? CANCEL_ACTION_CLASS
                      : undefined
                }
                disabled={mutation.isPending}
              >
                {t("confirm")}
              </Button>
              <Button
                type="button"
                variant="outline"
                disabled={mutation.isPending}
                onClick={() => setAction(undefined)}
              >
                {t("back")}
              </Button>
            </div>
          </form>
        )}
        {mutation.isError && (
          <p role="alert" className="text-destructive">
            {String(mutation.error)}
          </p>
        )}
      </div>
    </>
  );
}
