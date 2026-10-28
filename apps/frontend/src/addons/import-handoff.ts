import { z } from "zod";
import type { OpenActivityImportRequest } from "@wealthfolio/addon-sdk";
import type { DraftActivity } from "@/pages/activity/import/context";
import { ActivityType } from "@/lib/constants";

const numeric = z.union([z.string(), z.number().finite()]).nullable().optional();
const activity = z.object({
  activityType: z.enum(ActivityType),
  date: z.union([z.string(), z.date()]).optional(),
  symbol: z.string().optional(),
  assetId: z.string().optional(),
  amount: numeric,
  quantity: numeric,
  unitPrice: numeric,
  fee: numeric,
  tax: numeric,
  fxRate: numeric,
  currency: z.string().optional(),
  subtype: z.string().optional(),
  isExternal: z.boolean().optional(),
  comment: z.string().optional(),
  symbolName: z.string().optional(),
  exchangeMic: z.string().optional(),
  quoteCcy: z.string().optional(),
  instrumentType: z.string().optional(),
  quoteMode: z.string().optional(),
  providerId: z.string().optional(),
  providerSymbol: z.string().optional(),
});
const requestSchema = z.object({
  sourceLabel: z.string().min(1),
  accountId: z.string().min(1),
  rows: z
    .array(
      z.object({
        activity,
        sourceLine: z.number().int().positive(),
        rawRow: z.array(z.string()),
        excluded: z.boolean(),
      }),
    )
    .min(1),
});

export interface ImportHandoff {
  addonId: string;
  sourceLabel: string;
  accountId: string;
  returnRoute: string;
  drafts: DraftActivity[];
}

let pending: ImportHandoff | undefined;

export function prepareImportHandoff(
  addonId: string,
  input: OpenActivityImportRequest,
  returnRoute: string,
) {
  const request = requestSchema.parse(input);
  pending = {
    addonId,
    sourceLabel: request.sourceLabel,
    accountId: request.accountId,
    returnRoute,
    drafts: request.rows.map(({ activity: row, sourceLine, rawRow, excluded }, rowIndex) => ({
      ...row,
      amount: row.amount == null ? row.amount : String(row.amount),
      quantity: row.quantity == null ? row.quantity : String(row.quantity),
      unitPrice: row.unitPrice == null ? row.unitPrice : String(row.unitPrice),
      fee: row.fee == null ? row.fee : String(row.fee),
      tax: row.tax == null ? row.tax : String(row.tax),
      fxRate: row.fxRate == null ? row.fxRate : String(row.fxRate),
      rowIndex,
      rawRow,
      sourceLine,
      activityDate: row.date instanceof Date ? row.date.toISOString() : (row.date ?? ""),
      accountId: request.accountId,
      currency: row.currency ?? "",
      currencySource: "csv",
      status: excluded ? "skipped" : "valid",
      skipReason: excluded ? "Manual" : undefined,
      errors: {},
      warnings: {},
      isEdited: false,
      forceImport: false,
    })),
  };
}

export function getImportHandoff() {
  return pending;
}
export function clearImportHandoff(addonId?: string) {
  if (!addonId || pending?.addonId === addonId) pending = undefined;
}
