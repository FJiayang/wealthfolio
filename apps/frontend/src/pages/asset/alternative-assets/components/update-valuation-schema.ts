import type { TFunction } from "i18next";
import i18n from "i18next";
import { invalidTypeError, requiredError } from "@/lib/zod-errors";
import * as z from "zod";

/**
 * Zod schema for the update valuation form
 */
export const createUpdateValuationSchema = (t: TFunction) =>
  z.object({
    // New value - allow zero (e.g. a paid-off liability) but not negative
    value: z.coerce
      .number({ error: invalidTypeError(t("asset:updateValuation.validation.number")) })
      .min(0, t("asset:updateValuation.validation.non_negative")),

    // As of date - required
    date: z.date({ error: requiredError(t("activity:validation.date_is_required")) }),

    // Optional notes
    notes: z.string().max(500, t("asset:updateValuation.validation.notes")).optional(),
  });

export const updateValuationSchema = createUpdateValuationSchema(i18n.t.bind(i18n));

export type UpdateValuationFormValues = z.infer<ReturnType<typeof createUpdateValuationSchema>>;

/**
 * Get default form values for the update valuation form
 */
export const getUpdateValuationDefaultValues = (
  currentValue?: string,
): UpdateValuationFormValues => ({
  value: currentValue ? parseFloat(currentValue) : 0,
  date: new Date(),
  notes: "",
});
