import { fromDate, toCalendarDateTime } from "@internationalized/date";
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
