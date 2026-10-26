import { CalendarDate, fromDate, toCalendarDateTime } from "@internationalized/date";
import { parseDateTimeInTimezone } from "@wealthfolio/ui";
export function inputTime(value: string, timezone: string): string {
  return toCalendarDateTime(fromDate(new Date(value), timezone))
    .toString()
    .slice(0, 16);
}
export function instantTime(value: string, timezone: string): string {
  const parsed = parseDateTimeInTimezone(value, timezone);
  if (!parsed || Number.isNaN(parsed.getTime())) throw new Error("Invalid date/time");
  return parsed.toISOString();
}

export function pickerTime(value: Date | undefined, timezone: string): string {
  return value ? inputTime(value.toISOString(), timezone) : "";
}

// Calendar pickers return browser-local dates; interpret their day in the configured timezone.
export function completionDateBounds(
  range: { from?: Date; to?: Date } | undefined,
  timezone: string,
) {
  const day = (value: Date) =>
    new CalendarDate(value.getFullYear(), value.getMonth() + 1, value.getDate());
  return {
    expectedCompletionFrom: range?.from
      ? day(range.from).toDate(timezone).toISOString()
      : undefined,
    expectedCompletionBefore: range?.to
      ? day(range.to).add({ days: 1 }).toDate(timezone).toISOString()
      : undefined,
  };
}
