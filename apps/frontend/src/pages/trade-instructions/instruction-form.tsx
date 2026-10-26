import { useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { createTradeInstruction, type NewTradeInstruction } from "@/adapters";
import { AccountSelector } from "@/components/account-selector";
import TickerSearch from "@/components/ticker-search";
import { buildAssetResolutionInputFromSearchResult } from "@/lib/asset-resolution-input";
import type { Account, SymbolSearchResult } from "@/lib/types";
import { Button } from "@wealthfolio/ui/components/ui/button";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { Label } from "@wealthfolio/ui/components/ui/label";
import { Textarea } from "@wealthfolio/ui/components/ui/textarea";
import { useLocalizationSettings } from "@wealthfolio/ui";
import { instantTime } from "./time";
import { instructionKey } from "./use-trade-instructions";

const positiveDecimal = z
  .string()
  .regex(/^\d+(\.\d+)?$/)
  .refine((v) => /[1-9]/.test(v));
const schema = z.object({
  side: z.enum(["BUY", "SELL"]),
  sizeMode: z.enum(["quantity", "amount"]),
  size: positiveDecimal,
  amountCurrency: z.string(),
  limitPrice: z.union([z.literal(""), positiveDecimal]),
  rationale: z.string().max(8000),
  expectedCompletionAt: z.string(),
});
type Fields = z.infer<typeof schema>;
export function InstructionForm({ onSaved }: { onSaved: () => void }) {
  const { t } = useTranslation("trade-instructions");
  const { timezone = "UTC" } = useLocalizationSettings();
  const [account, setAccount] = useState<Account>();
  const [asset, setAsset] = useState<SymbolSearchResult>();
  const [selectionError, setSelectionError] = useState(false);
  // Preserve the same key for transport retries, but never reuse it for edited input.
  const [submission, setSubmission] = useState<{ fingerprint: string; requestId: string }>();
  const queryClient = useQueryClient();
  const {
    register,
    handleSubmit,
    watch,
    formState: { errors },
  } = useForm<Fields>({
    resolver: zodResolver(schema),
    defaultValues: {
      side: "BUY",
      sizeMode: "quantity",
      size: "",
      amountCurrency: "",
      limitPrice: "",
      rationale: "",
      expectedCompletionAt: "",
    },
  });
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
      accountId: account.id,
      asset: buildAssetResolutionInputFromSearchResult(asset),
      side: values.side,
      ...(values.sizeMode === "quantity"
        ? { quantity: values.size }
        : {
            amount: values.size,
            amountCurrency: values.amountCurrency.trim() || account.currency,
          }),
      ...(values.limitPrice ? { limitPrice: values.limitPrice } : {}),
      ...(values.rationale.trim() ? { rationale: values.rationale.trim() } : {}),
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
    <form onSubmit={submit} className="space-y-4">
      <fieldset disabled={mutation.isPending} className="space-y-4">
        <div>
          <Label>{t("account")}</Label>
          <AccountSelector
            selectedAccount={account}
            setSelectedAccount={setAccount}
            variant="form"
            accountPurpose="holdings"
            filterActive
            includePortfolio={false}
          />
        </div>
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
        <fieldset className="flex gap-4">
          <legend className="mb-2 text-sm font-medium">{t("side")}</legend>
          {(["BUY", "SELL"] as const).map((side) => (
            <label key={side} className="flex items-center gap-2">
              <input type="radio" value={side} {...register("side")} />
              {t(side)}
            </label>
          ))}
        </fieldset>
        <fieldset className="flex gap-4">
          <legend className="mb-2 text-sm font-medium">{t("size")}</legend>
          {(["quantity", "amount"] as const).map((mode) => (
            <label key={mode} className="flex items-center gap-2">
              <input type="radio" value={mode} {...register("sizeMode")} />
              {t(mode)}
            </label>
          ))}
        </fieldset>
        <div>
          <Label htmlFor="instruction-size">{t(watch("sizeMode"))}</Label>
          <Input id="instruction-size" inputMode="decimal" {...register("size")} />
        </div>
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
        <div>
          <Label htmlFor="instruction-limit">
            {t("limitPrice")} {asset?.currency && `(${asset.currency})`}
          </Label>
          <Input id="instruction-limit" inputMode="decimal" {...register("limitPrice")} />
          <p className="text-muted-foreground text-xs">{t("limitHint")}</p>
        </div>
        <div>
          <Label htmlFor="instruction-expected">
            {t("expectedCompletionAt")} ({timezone})
          </Label>
          <Input
            id="instruction-expected"
            type="datetime-local"
            {...register("expectedCompletionAt")}
          />
        </div>
        <div>
          <Label htmlFor="instruction-rationale">{t("rationale")}</Label>
          <Textarea id="instruction-rationale" {...register("rationale")} />
        </div>
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
      <Button type="submit" disabled={mutation.isPending}>
        {t("create")}
      </Button>
    </form>
  );
}
