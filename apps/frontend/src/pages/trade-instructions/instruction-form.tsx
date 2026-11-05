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
  type InstructionSide,
  type InstructionAssetSnapshot,
} from "@/adapters";
import { useAccounts } from "@/hooks/use-accounts";
import TickerSearch from "@/components/ticker-search";
import { buildAssetResolutionInputFromSearchResult } from "@/lib/asset-resolution-input";
import { accountSupportsPurpose, isLiabilityAccountType } from "@/lib/constants";
import { getActivityRestrictionLevel, restrictionAllowsType } from "@/lib/activity-restrictions";
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
  transferMode: z.enum(["cash", "securities"]),
  isExternal: z.boolean(),
  direction: z.enum(["in", "out"]),
  toAccountId: z.string(),
  destinationCurrency: z.string(),
  destinationAmount: z.union([z.literal(""), positiveDecimal]),
  unitPrice: z.union([z.literal(""), positiveDecimal]),
  costCurrency: z.string(),
  toQuantity: z.union([z.literal(""), positiveDecimal]),
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
  side: InstructionSide;
  onSaved: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation("trade-instructions");
  const { timezone = "UTC" } = useLocalizationSettings();
  const { accounts: allAccounts } = useAccounts({ filterActive: true });
  const initial = instruction?.side === side ? instruction : undefined;
  const searchResult = (id: string, snapshot: InstructionAssetSnapshot): SymbolSearchResult => ({
    symbol: snapshot.symbol ?? "",
    shortName: snapshot.name ?? "",
    longName: snapshot.name ?? "",
    exchange: snapshot.exchangeMic ?? "",
    exchangeMic: snapshot.exchangeMic ?? undefined,
    currency: snapshot.quoteCcy,
    quoteType: snapshot.instrumentType ?? "",
    index: id,
    score: 1,
    typeDisplay: snapshot.instrumentType ?? "",
    existingAssetId: id,
    isExisting: true,
  });
  const [asset, setAsset] = useState<SymbolSearchResult | undefined>(() =>
    initial?.assetId && initial.assetSnapshot
      ? searchResult(initial.assetId, initial.assetSnapshot)
      : undefined,
  );
  const [toAsset, setToAsset] = useState<SymbolSearchResult | undefined>(() =>
    initial?.conversion
      ? searchResult(initial.conversion.toAssetId, initial.conversion.toAssetSnapshot)
      : undefined,
  );
  const [selectionError, setSelectionError] = useState(false);
  // Preserve the same key for transport retries, but never reuse it for edited input.
  const [submission, setSubmission] = useState<{ fingerprint: string; requestId: string }>();
  const queryClient = useQueryClient();
  const form = useForm<Fields>({
    resolver: zodResolver(schema),
    defaultValues: {
      accountId: initial?.accountId ?? "",
      sizeMode: initial?.amount !== null && initial?.amount !== undefined ? "amount" : "quantity",
      size: initial?.quantity ?? initial?.amount ?? "",
      amountCurrency: initial?.amountCurrency ?? "",
      limitPrice: initial?.limitPrice ?? "",
      notes: initial?.notes ?? "",
      expectedCompletionAt: initial?.expectedCompletionAt
        ? inputTime(initial.expectedCompletionAt, timezone)
        : "",
      transferMode: initial?.transfer?.mode ?? "cash",
      isExternal: initial?.transfer?.isExternal ?? false,
      direction: initial?.transfer?.direction ?? "in",
      toAccountId: initial?.transfer?.toAccountId ?? "",
      destinationCurrency: initial?.transfer?.destinationCurrency ?? "",
      destinationAmount: initial?.transfer?.destinationAmount ?? "",
      unitPrice: initial?.transfer?.unitPrice ?? "",
      costCurrency: initial?.transfer?.costCurrency ?? "",
      toQuantity: initial?.conversion?.toQuantity ?? "",
    },
  });
  const {
    setValue,
    handleSubmit,
    watch,
    formState: { errors, dirtyFields },
  } = form;
  const trade = side === "BUY" || side === "SELL";
  const cash =
    side === "DEPOSIT" ||
    side === "WITHDRAWAL" ||
    (side === "TRANSFER" && watch("transferMode") === "cash");
  const needsAsset = !cash;
  const accounts = allAccounts.filter((item) => {
    if (item.isArchived) return false;
    if (side !== "TRANSFER") return accountSupportsPurpose(item, "holdings");
    if (isLiabilityAccountType(item.accountType)) return cash;
    return restrictionAllowsType(getActivityRestrictionLevel(item), "TRANSFER");
  });
  const accountOptions = accounts.map((item) => ({
    value: item.id,
    label: item.name,
    currency: item.currency,
    accountType: item.accountType,
  }));
  const account = accounts.find((item) => item.id === watch("accountId"));
  const mutation = useMutation({
    mutationFn: (input: NewTradeInstruction) => {
      if (!instruction) return createTradeInstruction(input);
      return updateTradeInstruction(instruction.id, {
        expectedVersion: instruction.version,
        accountId: input.accountId,
        asset: input.asset,
        side: input.side,
        transfer: input.transfer,
        conversion: input.conversion,
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
    if (
      !account ||
      (needsAsset && !asset) ||
      (side === "CONVERSION" && !toAsset) ||
      (side === "TRANSFER" &&
        !values.isExternal &&
        !accounts.some((a) => a.id === values.toAccountId))
    ) {
      setSelectionError(true);
      return;
    }
    setSelectionError(false);
    const input: Omit<NewTradeInstruction, "requestId"> = {
      goalId,
      accountId: account.id,
      ...(needsAsset && asset
        ? {
            asset: asset.existingAssetId
              ? { id: asset.existingAssetId }
              : buildAssetResolutionInputFromSearchResult(asset),
          }
        : {}),
      side,
      ...((trade ? values.sizeMode === "quantity" : !cash)
        ? { quantity: values.size }
        : {
            amount: values.size,
            amountCurrency: values.amountCurrency.trim() || account.currency,
          }),
      ...(trade && values.limitPrice ? { limitPrice: values.limitPrice } : {}),
      ...(side === "TRANSFER"
        ? {
            transfer: {
              mode: values.transferMode,
              isExternal: values.isExternal,
              ...(values.isExternal
                ? { direction: values.direction }
                : {
                    toAccountId: values.toAccountId,
                    ...(cash
                      ? {
                          destinationCurrency:
                            values.destinationCurrency.trim() ||
                            accounts.find((a) => a.id === values.toAccountId)?.currency,
                          ...(values.destinationAmount
                            ? { destinationAmount: values.destinationAmount }
                            : {}),
                        }
                      : {}),
                  }),
              ...(!cash && values.isExternal && values.direction === "in" && values.unitPrice
                ? {
                    unitPrice: values.unitPrice,
                    costCurrency: values.costCurrency.trim() || asset?.currency,
                  }
                : {}),
            },
          }
        : {}),
      ...(side === "CONVERSION" && toAsset
        ? {
            conversion: {
              toAsset: toAsset.existingAssetId
                ? { id: toAsset.existingAssetId }
                : buildAssetResolutionInputFromSearchResult(toAsset),
              ...(values.toQuantity ? { toQuantity: values.toQuantity } : {}),
            },
          }
        : {}),
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
          {side === "TRANSFER" && (
            <FormSection title={t("TRANSFER")}>
              <RadioGroup
                aria-label={t("transferMode")}
                value={watch("transferMode")}
                onValueChange={(value) => {
                  setValue("transferMode", value as Fields["transferMode"]);
                  setValue("accountId", "");
                  setValue("toAccountId", "");
                  setValue("size", "");
                  setValue("destinationAmount", "");
                  setValue("unitPrice", "");
                }}
              >
                {(["cash", "securities"] as const).map((value) => (
                  <div key={value} className="flex items-center gap-2">
                    <RadioGroupItem id={`transfer-${value}`} value={value} />
                    <Label htmlFor={`transfer-${value}`}>{t(value)}</Label>
                  </div>
                ))}
              </RadioGroup>
              <RadioGroup
                aria-label={t("transferScope")}
                value={watch("isExternal") ? "external" : "internal"}
                onValueChange={(value) => {
                  setValue("isExternal", value === "external");
                  setValue("destinationAmount", "");
                  setValue("unitPrice", "");
                }}
              >
                {(["internal", "external"] as const).map((value) => (
                  <div key={value} className="flex items-center gap-2">
                    <RadioGroupItem id={`transfer-${value}`} value={value} />
                    <Label htmlFor={`transfer-${value}`}>{t(value)}</Label>
                  </div>
                ))}
              </RadioGroup>
              {watch("isExternal") && (
                <RadioGroup
                  aria-label={t("direction")}
                  value={watch("direction")}
                  onValueChange={(value) => {
                    setValue("direction", value as Fields["direction"]);
                    setValue("unitPrice", "");
                    setValue("costCurrency", "");
                  }}
                >
                  {(["in", "out"] as const).map((value) => (
                    <div key={value} className="flex items-center gap-2">
                      <RadioGroupItem id={`transfer-${value}`} value={value} />
                      <Label htmlFor={`transfer-${value}`}>{t(value)}</Label>
                    </div>
                  ))}
                </RadioGroup>
              )}
            </FormSection>
          )}
          <FormSection title={t("activity:form.section_asset_account")}>
            {needsAsset && (
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
            )}
            <AccountSelect
              name="accountId"
              accounts={accountOptions}
              label={side === "TRANSFER" && !watch("isExternal") ? t("fromAccount") : t("account")}
            />
            {side === "TRANSFER" && !watch("isExternal") && (
              <AccountSelect name="toAccountId" accounts={accountOptions} label={t("toAccount")} />
            )}
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
            {trade && (
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
            )}
            <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
              <div>
                <Label htmlFor="instruction-size">
                  {t(trade ? watch("sizeMode") : cash ? "amount" : "quantity")}
                </Label>
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
              {trade && (
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
              )}
            </div>
            {trade && <p className="text-muted-foreground text-xs">{t("limitHint")}</p>}
            {(cash || (trade && watch("sizeMode") === "amount")) && (
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
          {side === "TRANSFER" && cash && !watch("isExternal") && (
            <FormSection title={t("destination")}>
              <Label htmlFor="destination-currency">{t("destinationCurrency")}</Label>
              <CurrencyInput
                id="destination-currency"
                aria-label={t("destinationCurrency")}
                value={
                  watch("destinationCurrency") ||
                  accounts.find((a) => a.id === watch("toAccountId"))?.currency
                }
                onChange={(value) => {
                  setValue("destinationCurrency", value, { shouldDirty: true });
                  setValue("destinationAmount", "");
                }}
              />
              <Label htmlFor="destination-amount">{t("destinationAmount")}</Label>
              <MoneyInput
                id="destination-amount"
                maxDecimalPlaces={28}
                thousandSeparator
                value={watch("destinationAmount")}
                onValueStringChange={(value) =>
                  setValue("destinationAmount", value, { shouldDirty: true })
                }
              />
            </FormSection>
          )}
          {side === "TRANSFER" && !cash && watch("isExternal") && watch("direction") === "in" && (
            <FormSection title={t("costBasis")}>
              <Label htmlFor="instruction-cost">{t("unitPrice")}</Label>
              <MoneyInput
                id="instruction-cost"
                maxDecimalPlaces={28}
                thousandSeparator
                value={watch("unitPrice")}
                onValueStringChange={(value) => setValue("unitPrice", value, { shouldDirty: true })}
              />
              <Label htmlFor="cost-currency">{t("currency")}</Label>
              <CurrencyInput
                id="cost-currency"
                aria-label={t("costCurrency")}
                value={watch("costCurrency") || asset?.currency}
                onChange={(value) => setValue("costCurrency", value, { shouldDirty: true })}
              />
            </FormSection>
          )}
          {side === "CONVERSION" && (
            <FormSection title={t("conversionTo")}>
              <Label>{t("asset")}</Label>
              <TickerSearch
                selectedResult={toAsset}
                value={toAsset?.symbol}
                defaultCurrency={account?.currency}
                onSelectResult={(_symbol, result) => setToAsset(result)}
                onClear={() => setToAsset(undefined)}
              />
              <Label htmlFor="conversion-quantity">{t("toQuantity")}</Label>
              <MoneyInput
                id="conversion-quantity"
                maxDecimalPlaces={28}
                thousandSeparator
                value={watch("toQuantity")}
                onValueStringChange={(value) =>
                  setValue("toQuantity", value, { shouldDirty: true })
                }
              />
            </FormSection>
          )}
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
              : trade
                ? t(
                    side === "BUY"
                      ? "activity:form.button_add_buy"
                      : "activity:form.button_add_sell",
                  )
                : t("create")}
          </Button>
        </div>
      </form>
    </FormProvider>
  );
}
