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
import { DatePickerInput } from "@wealthfolio/ui";
import { Label } from "@wealthfolio/ui/components/ui/label";
import { Textarea } from "@wealthfolio/ui/components/ui/textarea";
import { useNumberFormatting, useDateFormatting, useLocalizationSettings } from "@wealthfolio/ui";
import { inputTime, instantTime, pickerTime } from "./time";
import { instructionKey, useTradeInstruction } from "./use-trade-instructions";

type Action = "start" | "complete" | "cancel" | "correct";
export function InstructionDetail({ id }: { id: string }) {
  const { t } = useTranslation("trade-instructions");
  const { timezone = "UTC" } = useLocalizationSettings();
  const { formatDateTime } = useDateFormatting();
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
  if (query.isPending) return <p>{t("loading")}</p>;
  if (query.isError) return <p role="alert">{String(query.error)}</p>;
  const row = query.data;
  const begin = (kind: Action) => {
    mutation.reset();
    setNote("");
    setTime(kind === "correct" && row.completedAt ? inputTime(row.completedAt, timezone) : "");
    setAction({ kind, row });
  };
  const date = (value: string | null) => (value ? formatDateTime(value) : t("notSet"));
  const fields = [
    [t("account"), row.accountName],
    [
      t("asset"),
      [row.assetSnapshot.symbol, row.assetSnapshot.exchangeMic, row.assetSnapshot.name]
        .filter(Boolean)
        .join(" · "),
    ],
    [t("side"), t(row.side)],
    [t("status"), t(row.status)],
    [
      t(row.quantity ? "quantity" : "amount"),
      row.quantity
        ? formatQuantity(row.quantity)
        : `${formatQuantity(row.amount)} ${row.amountCurrency}`,
    ],
    [
      t("limitPrice"),
      row.limitPrice
        ? `${formatQuantity(row.limitPrice)} ${row.assetSnapshot.quoteCcy}`
        : t("notSet"),
    ],
    [t("activity:form.label_notes"), row.notes || t("notSet")],
    [t("expectedCompletionAt"), date(row.expectedCompletionAt)],
    [t("completedAt"), date(row.completedAt)],
    [t("closedAt"), date(row.closedAt)],
    [t("createdAt"), date(row.createdAt)],
    [t("source"), row.source],
    [t("resolutionNote"), row.resolutionNote || t("notSet")],
  ];
  const late =
    row.expectedCompletionAt &&
    row.completedAt &&
    Date.parse(row.completedAt) > Date.parse(row.expectedCompletionAt);
  return (
    <div className="space-y-5">
      <p className="text-muted-foreground text-sm">{t("independent")}</p>
      <dl className="space-y-3">
        {fields.map(([label, value]) => (
          <div key={label}>
            <dt className="text-muted-foreground text-sm">{label}</dt>
            <dd className="whitespace-pre-wrap break-words">{value}</dd>
          </div>
        ))}
      </dl>
      {row.completedAt && row.expectedCompletionAt && (
        <p>{t(late ? "completedLate" : "completedOnTime")}</p>
      )}
      {!action && (
        <div className="flex flex-wrap gap-2">
          {row.status === "PENDING" && (
            <Button disabled={mutation.isPending} onClick={() => begin("start")}>
              {t("start")}
            </Button>
          )}
          {(row.status === "PENDING" || row.status === "IN_PROGRESS") && (
            <>
              <Button disabled={mutation.isPending} onClick={() => begin("complete")}>
                {t("complete")}
              </Button>
              <Button
                variant="outline"
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
          className="space-y-3 rounded-md border p-3"
          onSubmit={(event) => {
            event.preventDefault();
            mutation.mutate();
          }}
        >
          <p className="font-medium">{t(action.kind)}</p>
          {(action.kind === "complete" || action.kind === "cancel") && (
            <div>
              <Label htmlFor="resolution-note">{t("resolutionNote")}</Label>
              <Textarea
                id="resolution-note"
                required
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
            <Button type="submit" disabled={mutation.isPending}>
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
  );
}
