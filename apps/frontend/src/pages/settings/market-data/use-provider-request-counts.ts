import { useQuery } from "@tanstack/react-query";
import { getProviderRequestCounts } from "@/adapters";
import type { ProviderRequestCounts } from "@/adapters/shared/market-data";
import { selectedProfileId } from "@/features/profiles/session";
import { QueryKeys } from "@/lib/query-keys";

export function useProviderRequestCounts(days: number, timezone = "Asia/Shanghai") {
  return useQuery({
    queryKey: [QueryKeys.PROVIDER_REQUEST_COUNTS, selectedProfileId(), timezone, days],
    queryFn: () => getProviderRequestCounts(days),
    staleTime: 30_000,
    refetchInterval: 30_000,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
  });
}

export function requestTrackingDay(data: ProviderRequestCounts): string {
  const parts = new Intl.DateTimeFormat("en-US-u-ca-gregory-nu-latn", {
    timeZone: data.timezone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).formatToParts(new Date(data.trackingSince));
  const part = (type: string) => parts.find((value) => value.type === type)?.value;
  return `${part("year")}-${part("month")}-${part("day")}`;
}

export function requestCountDays(data: ProviderRequestCounts, providerId: string) {
  const counts = new Map(
    data.rows
      .filter((row) => row.providerId === providerId)
      .map((row) => [row.day, row.requestCount]),
  );
  const trackingDay = requestTrackingDay(data);
  const result: { day: string; count: number | null; partial: boolean }[] = [];
  // These are calendar labels, not instants in the selected statistics timezone.
  for (
    let time = Date.parse(`${data.endDate}T00:00:00Z`);
    time >= Date.parse(`${data.startDate}T00:00:00Z`);
    time -= 86_400_000
  ) {
    const day = new Date(time).toISOString().slice(0, 10);
    result.push({
      day,
      count: day < trackingDay ? null : (counts.get(day) ?? 0),
      partial: day === trackingDay || day === data.endDate,
    });
  }
  return result;
}

export function todayRequestCount(data: ProviderRequestCounts | undefined, providerId: string) {
  if (!data || data.endDate < requestTrackingDay(data)) return undefined;
  return (
    data.rows.find((row) => row.providerId === providerId && row.day === data.endDate)
      ?.requestCount ?? 0
  );
}
