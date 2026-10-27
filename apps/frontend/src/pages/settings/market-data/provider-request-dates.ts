import { parseDate } from "@internationalized/date";

// Calendar dates belong to the statistics timezone, not the browser timezone.
export function providerRequestDateBounds(from: string, to: string, timezone: string) {
  try {
    const first = parseDate(from);
    const last = parseDate(to);
    if (first.compare(last) > 0) return null;
    return {
      start: first.toDate(timezone).toISOString(),
      end: last.add({ days: 1 }).toDate(timezone).toISOString(),
    };
  } catch {
    return null;
  }
}
