import { useState } from "react";
import { FormProvider, useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  createTradeInstruction,
  updateTradeInstruction,
  type TradeInstruction,
  type NewTradeInstruction,
} from "@/adapters";
import { useAccounts } from "@/hooks/use-accounts";
import TickerSearch from "@/components/ticker-search";
import { buildAssetResolutionInputFromSearchResult } from "@/lib/asset-resolution-input";
import type { SymbolSearchResult } from "@/lib/types";
import { Button } from "@wealthfolio/ui/components/ui/button";
import { CurrencyInput, MoneyInput } from "@wealthfolio/ui/components/financial";
import { RadioGroup, RadioGroupItem } from "@wealthfolio/ui/components/ui/radio-group";
import { Label } from "@wealthfolio/ui/components/ui/label";
import { DatePickerInput, Icons, Spinner, useLocalizationSettings } from "@wealthfolio/ui";
import {
  AccountSelect,
  AdvancedOptionsSection,
  FormSection,
  NotesInput,
} from "@/pages/activity/components/forms/fields";
import { inputTime, instantTime, pickerTime } from "./time";
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
  instruction,
  side,
  onSaved,
  onCancel,
}: {
  goalId: string;
  instruction?: TradeInstruction;
  side: "BUY" | "SELL";
  onSaved: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation("trade-instructions");
  const { timezone = "UTC" } = useLocalizationSettings();
  const { accounts } = useAccounts({ filterActive: true, accountPurpose: "holdings" });
  const [asset, setAsset] = useState<SymbolSearchResult | undefined>(() =>
    instruction
      ? {
          symbol: instruction.assetSnapshot.symbol ?? "",
          shortName: instruction.assetSnapshot.name ?? "",
          longName: instruction.assetSnapshot.name ?? "",
          exchange: instruction.assetSnapshot.exchangeMic ?? "",
          exchangeMic: instruction.assetSnapshot.exchangeMic ?? undefined,
          currency: instruction.assetSnapshot.quoteCcy,
          quoteType: instruction.assetSnapshot.instrumentType ?? "",
          index: instruction.assetId,
          score: 1,
          typeDisplay: instruction.assetSnapshot.instrumentType ?? "",
          existingAssetId: instruction.assetId,
          isExisting: true,
        }
      : undefined,
  );
  const [selectionError, setSelectionError] = useState(false);
  // Preserve the same key for transport retries, but never reuse it for edited input.
  const [submission, setSubmission] = useState<{ fingerprint: string; requestId: string }>();
  const queryClient = useQueryClient();
  const form = useForm<Fields>({
    resolver: zodResolver(schema),
    defaultValues: {
      accountId: instruction?.accountId ?? "",
      sizeMode:
        instruction?.amount !== null && instruction?.amount !== undefined ? "amount" : "quantity",
      size: instruction?.quantity ?? instruction?.amount ?? "",
      amountCurrency: instruction?.amountCurrency ?? "",
      limitPrice: instruction?.limitPrice ?? "",
      notes: instruction?.notes ?? "",
      expectedCompletionAt: instruction?.expectedCompletionAt
        ? inputTime(instruction.expectedCompletionAt, timezone)
        : "",
    },
  });
  const {
    setValue,
    handleSubmit,
    watch,
    formState: { errors, dirtyFields },
  } = form;
  const account = accounts.find((item) => item.id === watch("accountId"));
  const mutation = useMutation({
    mutationFn: (input: NewTradeInstruction) => {
      if (!instruction) return createTradeInstruction(input);
      return updateTradeInstruction(instruction.id, {
        expectedVersion: instruction.version,
        accountId: input.accountId,
        asset: input.asset,
        side: input.side,
        quantity: input.quantity,
        amount: input.amount,
        amountCurrency: input.amountCurrency,
        limitPrice: input.limitPrice,
        notes: input.notes,
        expectedCompletionAt: input.expectedCompletionAt,
      });
    },
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
      asset:
        instruction && asset.existingAssetId === instruction.assetId
          ? { id: instruction.assetId }
          : buildAssetResolutionInputFromSearchResult(asset),
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
        ? {
            expectedCompletionAt:
              instruction?.expectedCompletionAt && !dirtyFields.expectedCompletionAt
                ? instruction.expectedCompletionAt
                : instantTime(values.expectedCompletionAt, timezone),
          }
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
            <RadioGroup
              aria-label={t("size")}
              className="flex gap-4"
              value={watch("sizeMode")}
              disabled={mutation.isPending}
              onValueChange={(value) =>
                setValue("sizeMode", value as Fields["sizeMode"], { shouldDirty: true })
              }
            >
              {(["quantity", "amount"] as const).map((mode) => (
                <div key={mode} className="flex items-center gap-2">
                  <RadioGroupItem id={`instruction-${mode}`} value={mode} />
                  <Label htmlFor={`instruction-${mode}`}>{t(mode)}</Label>
                </div>
              ))}
            </RadioGroup>
            <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
              <div>
                <Label htmlFor="instruction-size">{t(watch("sizeMode"))}</Label>
                <MoneyInput
                  id="instruction-size"
                  className="tabular-nums"
                  maxDecimalPlaces={28}
                  thousandSeparator
                  value={watch("size")}
                  onValueStringChange={(value) => setValue("size", value, { shouldDirty: true })}
                  disabled={mutation.isPending}
                />
              </div>
              <div>
                <Label htmlFor="instruction-limit">
                  {t("limitPrice")} {asset?.currency && `(${asset.currency})`}
                </Label>
                <MoneyInput
                  id="instruction-limit"
                  className="tabular-nums"
                  maxDecimalPlaces={28}
                  thousandSeparator
                  value={watch("limitPrice")}
                  onValueStringChange={(value) =>
                    setValue("limitPrice", value, { shouldDirty: true })
                  }
                  disabled={mutation.isPending}
                />
              </div>
            </div>
            <p className="text-muted-foreground text-xs">{t("limitHint")}</p>
            {watch("sizeMode") === "amount" && (
              <div>
                <Label htmlFor="instruction-currency">{t("currency")}</Label>
                <CurrencyInput
                  id="instruction-currency"
                  aria-label={t("currency")}
                  value={watch("amountCurrency") || account?.currency}
                  onChange={(value) => setValue("amountCurrency", value, { shouldDirty: true })}
                  disabled={mutation.isPending}
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
            {instruction
              ? t("common:save")
              : t(
                  side === "BUY" ? "activity:form.button_add_buy" : "activity:form.button_add_sell",
                )}
          </Button>
        </div>
      </form>
    </FormProvider>
  );
}
