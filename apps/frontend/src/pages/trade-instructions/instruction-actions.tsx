import { useId, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  Button,
  Icons,
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  AlertDialog,
  AlertDialogContent,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogCancel,
  DatePickerInput,
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  Label,
  Textarea,
  useLocalizationSettings,
} from "@wealthfolio/ui";
import {
  deleteTradeInstruction,
  cancelTradeInstruction,
  completeTradeInstruction,
  startTradeInstruction,
  type TradeInstruction,
} from "@/adapters";
import { toast } from "@wealthfolio/ui/components/ui/use-toast";
import { instructionKey } from "./use-trade-instructions";
import { inputTime, instantTime, pickerTime } from "./time";

type QuickAction = "start" | "complete" | "cancel" | "delete";
export function InstructionActions({
  row,
  onOpen,
  onEdit,
}: {
  row: TradeInstruction;
  onOpen: (id: string) => void;
  onEdit: (id: string) => void;
}) {
  const { t } = useTranslation("trade-instructions");
  const { timezone = "UTC" } = useLocalizationSettings();
  const queryClient = useQueryClient();
  const fieldId = useId();
  const [action, setAction] = useState<"complete" | "cancel" | "delete">();
  const [note, setNote] = useState("");
  const [time, setTime] = useState("");
  const mutation = useMutation({
    mutationFn: async ({ kind, version }: { kind: QuickAction; version: number }) => {
      if (kind === "delete") return deleteTradeInstruction(row.id, version);
      if (kind === "start") return startTradeInstruction(row.id, version);
      if (kind === "cancel") return cancelTradeInstruction(row.id, version, note);
      return completeTradeInstruction(row.id, {
        expectedVersion: version,
        resolutionNote: note,
        ...(time ? { completedAt: instantTime(time, timezone) } : {}),
      });
    },
    onSuccess: async () => {
      setAction(undefined);
      await queryClient.invalidateQueries({ queryKey: instructionKey });
    },
    onError: async (error, { kind }) => {
      toast({ title: t(kind), description: String(error), variant: "destructive" });
      await queryClient.invalidateQueries({ queryKey: instructionKey });
    },
  });
  // Keep the version selected when opening the form, so a concurrent edit cannot be overwritten.
  const [actionVersion, setActionVersion] = useState(row.version);
  const open = row.status === "PENDING" || row.status === "IN_PROGRESS";
  const begin = (kind: "complete" | "cancel" | "delete") => {
    mutation.reset();
    setNote("");
    setTime(kind === "complete" ? inputTime(new Date().toISOString(), timezone) : "");
    setActionVersion(row.version);
    setAction(kind);
  };
  return (
    <>
      <div className="flex h-full items-center gap-1">
        <Button
          size="xs"
          variant="ghost"
          className="min-w-0 flex-auto px-1 text-xs"
          title={t("startShort")}
          disabled={row.status !== "PENDING" || mutation.isPending}
          onClick={() => mutation.mutate({ kind: "start", version: row.version })}
        >
          <span className="truncate">{t("startShort")}</span>
        </Button>
        <Button
          size="xs"
          variant="ghost"
          className="text-success min-w-0 flex-auto px-1 text-xs"
          title={t("completeShort")}
          disabled={!open || mutation.isPending}
          onClick={() => begin("complete")}
        >
          <span className="truncate">{t("completeShort")}</span>
        </Button>
        <Button
          size="xs"
          variant="ghost"
          className="text-destructive min-w-0 flex-auto px-1 text-xs"
          title={t("cancelShort")}
          disabled={!open || mutation.isPending}
          onClick={() => begin("cancel")}
        >
          <span className="truncate">{t("cancelShort")}</span>
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6"
                aria-label={t("menu")}
                disabled={mutation.isPending}
              />
            }
          >
            <Icons.MoreVertical className="h-4 w-4" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onClick={() => onOpen(row.id)}>
              <Icons.Info className="me-2 h-4 w-4" />
              {t("detail")}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              disabled={row.status !== "PENDING"}
              title={row.status !== "PENDING" ? t("editPendingOnly") : undefined}
              onClick={() => onEdit(row.id)}
            >
              <Icons.Pencil className="me-2 h-4 w-4" />
              {t("edit")}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              className="text-destructive focus:text-destructive"
              onClick={() => begin("delete")}
            >
              <Icons.Trash className="me-2 h-4 w-4" />
              {t("delete")}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <AlertDialog
        open={action === "delete"}
        onOpenChange={(value) => {
          if (!value && !mutation.isPending) setAction(undefined);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("deleteTitle")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("deleteDescription", {
                symbol: row.assetSnapshot.symbol || row.assetSnapshot.name,
                account: row.accountName,
              })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          {mutation.isError && (
            <p role="alert" className="text-destructive text-sm">
              {String(mutation.error)}
            </p>
          )}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={mutation.isPending}>{t("back")}</AlertDialogCancel>
            <Button
              variant="destructive"
              disabled={mutation.isPending}
              onClick={() => mutation.mutate({ kind: "delete", version: actionVersion })}
            >
              {t("delete")}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <Dialog
        open={!!action && action !== "delete"}
        onOpenChange={(value) => {
          if (!value && !mutation.isPending) setAction(undefined);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{action ? t(action) : ""}</DialogTitle>
          </DialogHeader>
          <DialogDescription>
            {row.assetSnapshot.symbol || row.assetSnapshot.name} · {row.accountName}
          </DialogDescription>
          <form
            className="space-y-4"
            onSubmit={(event) => {
              event.preventDefault();
              if (action) mutation.mutate({ kind: action, version: actionVersion });
            }}
          >
            <div className="space-y-2">
              <Label htmlFor={`${fieldId}-note`}>
                {t(action === "complete" ? "completionNote" : "resolutionNote")}
              </Label>
              <Textarea
                id={`${fieldId}-note`}
                required={action === "cancel"}
                maxLength={4000}
                value={note}
                onChange={(event) => setNote(event.target.value)}
                disabled={mutation.isPending}
              />
            </div>
            {action === "complete" && (
              <div className="space-y-2">
                <Label htmlFor={`${fieldId}-time`}>
                  {t("completedAt")} ({timezone})
                </Label>
                <DatePickerInput
                  id={`${fieldId}-time`}
                  aria-label={`${t("completedAt")} (${timezone})`}
                  enableTime
                  timeZone={timezone}
                  timeGranularity="minute"
                  value={time || undefined}
                  onChange={(date) => setTime(pickerTime(date, timezone))}
                  disabled={mutation.isPending}
                />
                <p className="text-muted-foreground text-xs">{t("completionDefault")}</p>
              </div>
            )}
            {mutation.isError && (
              <p role="alert" className="text-destructive text-sm">
                {String(mutation.error)}
              </p>
            )}
            <div className="flex justify-end gap-2">
              <Button
                type="button"
                variant="outline"
                disabled={mutation.isPending}
                onClick={() => setAction(undefined)}
              >
                {t("back")}
              </Button>
              <Button type="submit" disabled={mutation.isPending}>
                {t("confirm")}
              </Button>
            </div>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}
