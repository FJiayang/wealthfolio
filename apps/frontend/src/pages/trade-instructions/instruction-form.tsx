import { useState } from "react";
import { FormProvider, useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { createTradeInstruction, type NewTradeInstruction } from "@/adapters";
import { useAccounts } from "@/hooks/use-accounts";
import TickerSearch from "@/components/ticker-search";
import { buildAssetResolutionInputFromSearchResult } from "@/lib/asset-resolution-input";
import type { SymbolSearchResult } from "@/lib/types";
import { Button } from "@wealthfolio/ui/components/ui/button";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { Label } from "@wealthfolio/ui/components/ui/label";
import { DatePickerInput, Icons, Spinner, useLocalizationSettings } from "@wealthfolio/ui";
import {
  AccountSelect,
  AdvancedOptionsSection,
  FormSection,
  NotesInput,
} from "@/pages/activity/components/forms/fields";
import { instantTime, pickerTime } from "./time";
import { instructionKey } from "./use-trade-instructions";

const positiveDecimal = z
  .string()
  .regex(/^\d+(\.\d+)?$/)
  .refine((v) => /[1-9]/.test(v));
const schema = z.object({
  accountId: z.string().min(1),
  sizeMode: z.enum(["quantity", "amount"]),
  size: positiveDecimal,
  amountCurrency: z.string(),
  limitPrice: z.union([z.literal(""), positiveDecimal]),
  notes: z.string().max(8000),
  expectedCompletionAt: z.string(),
});
type Fields = z.infer<typeof schema>;
export function InstructionForm({
  goalId,
  side,
  onSaved,
  onCancel,
}: {
  goalId: string;
  side: "BUY" | "SELL";
  onSaved: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation("trade-instructions");
  const { timezone = "UTC" } = useLocalizationSettings();
  const { accounts } = useAccounts({ filterActive: true, accountPurpose: "holdings" });
  const [asset, setAsset] = useState<SymbolSearchResult>();
  const [selectionError, setSelectionError] = useState(false);
  // Preserve the same key for transport retries, but never reuse it for edited input.
  const [submission, setSubmission] = useState<{ fingerprint: string; requestId: string }>();
  const queryClient = useQueryClient();
  const form = useForm<Fields>({
    resolver: zodResolver(schema),
    defaultValues: {
      accountId: "",
      sizeMode: "quantity",
      size: "",
      amountCurrency: "",
      limitPrice: "",
      notes: "",
      expectedCompletionAt: "",
    },
  });
  const {
    register,
    setValue,
    handleSubmit,
    watch,
    formState: { errors },
  } = form;
  const account = accounts.find((item) => item.id === watch("accountId"));
  const mutation = useMutation({
    mutationFn: createTradeInstruction,
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: instructionKey });
      onSaved();
    },
  });
  const submit = handleSubmit((values) => {
    if (!account || !asset) {
      setSelectionError(true);
      return;
    }
    setSelectionError(false);
    const input: Omit<NewTradeInstruction, "requestId"> = {
      goalId,
      accountId: account.id,
      asset: buildAssetResolutionInputFromSearchResult(asset),
      side,
      ...(values.sizeMode === "quantity"
        ? { quantity: values.size }
        : {
            amount: values.size,
            amountCurrency: values.amountCurrency.trim() || account.currency,
          }),
      ...(values.limitPrice ? { limitPrice: values.limitPrice } : {}),
      ...(values.notes.trim() ? { notes: values.notes.trim() } : {}),
      ...(values.expectedCompletionAt
        ? { expectedCompletionAt: instantTime(values.expectedCompletionAt, timezone) }
        : {}),
    };
    const fingerprint = JSON.stringify(input);
    const requestId =
      submission?.fingerprint === fingerprint ? submission.requestId : crypto.randomUUID();
    setSubmission({ fingerprint, requestId });
    mutation.mutate({ ...input, requestId });
  });
  return (
    <FormProvider {...form}>
      <form onSubmit={submit} className="space-y-4">
        <fieldset disabled={mutation.isPending} className="space-y-4">
          <FormSection title={t("activity:form.section_asset_account")}>
            <div>
              <Label>{t("asset")}</Label>
              <TickerSearch
                selectedResult={asset}
                value={asset?.symbol}
                defaultCurrency={account?.currency}
                onSelectResult={(_symbol, result) => setAsset(result)}
                onClear={() => setAsset(undefined)}
              />
            </div>
            <AccountSelect
              name="accountId"
              accounts={accounts.map((item) => ({
                value: item.id,
                label: item.name,
                currency: item.currency,
                accountType: item.accountType,
              }))}
            />
            <div>
              <Label htmlFor="instruction-expected">
                {t("expectedCompletionAt")} ({timezone})
              </Label>
              <DatePickerInput
                id="instruction-expected"
                aria-label={`${t("expectedCompletionAt")} (${timezone})`}
                data-testid="instruction-expected-picker"
                enableTime
                timeZone={timezone}
                timeGranularity="minute"
                value={watch("expectedCompletionAt") || undefined}
                onChange={(date) =>
                  setValue("expectedCompletionAt", pickerTime(date, timezone), {
                    shouldDirty: true,
                    shouldValidate: true,
                  })
                }
                disabled={mutation.isPending}
              />
            </div>
          </FormSection>
          <FormSection title={t("activity:form.section_trade")}>
            <fieldset className="flex gap-4">
              <legend className="sr-only">{t("size")}</legend>
              {(["quantity", "amount"] as const).map((mode) => (
                <label key={mode} className="flex items-center gap-2">
                  <input type="radio" value={mode} {...register("sizeMode")} />
                  {t(mode)}
                </label>
              ))}
            </fieldset>
            <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
              <div>
                <Label htmlFor="instruction-size">{t(watch("sizeMode"))}</Label>
                <Input
                  id="instruction-size"
                  inputMode="decimal"
                  className="text-end tabular-nums"
                  placeholder="0.00"
                  {...register("size")}
                />
              </div>
              <div>
                <Label htmlFor="instruction-limit">
                  {t("limitPrice")} {asset?.currency && `(${asset.currency})`}
                </Label>
                <Input
                  id="instruction-limit"
                  inputMode="decimal"
                  className="text-end tabular-nums"
                  placeholder="0.00"
                  {...register("limitPrice")}
                />
              </div>
            </div>
            <p className="text-muted-foreground text-xs">{t("limitHint")}</p>
            {watch("sizeMode") === "amount" && (
              <div>
                <Label htmlFor="instruction-currency">{t("currency")}</Label>
                <Input
                  id="instruction-currency"
                  placeholder={account?.currency}
                  {...register("amountCurrency")}
                />
              </div>
            )}
          </FormSection>
          <AdvancedOptionsSection
            title={t("activity:form.section_advanced_notes")}
            dashed
            showCurrency={false}
            showFxRate={false}
            showSubtype={false}
          >
            <NotesInput name="notes" />
          </AdvancedOptionsSection>
        </fieldset>
        {(selectionError || Object.keys(errors).length > 0) && (
          <p role="alert" className="text-destructive">
            {t("invalidInput")}
          </p>
        )}
        {mutation.isError && (
          <p role="alert" className="text-destructive">
            {String(mutation.error)}
          </p>
        )}
        <div className="flex justify-end gap-3 pt-2">
          <Button type="button" variant="outline" onClick={onCancel} disabled={mutation.isPending}>
            {t("common:cancel")}
          </Button>
          <Button type="submit" disabled={mutation.isPending}>
            {mutation.isPending ? (
              <Spinner className="me-2 h-4 w-4" />
            ) : (
              <Icons.Plus className="me-2 h-4 w-4" />
            )}
            {t(side === "BUY" ? "activity:form.button_add_buy" : "activity:form.button_add_sell")}
          </Button>
        </div>
      </form>
    </FormProvider>
  );
}
