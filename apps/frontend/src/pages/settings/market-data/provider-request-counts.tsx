import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useDateFormatting, useNumberFormatting } from "@wealthfolio/ui";
import { Button } from "@wealthfolio/ui/components/ui/button";
import { Label } from "@wealthfolio/ui/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@wealthfolio/ui/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@wealthfolio/ui/components/ui/table";
import { useSettingsContext } from "@/lib/settings-provider";
import { TimezoneInput } from "../general/timezone-input";
import { getSupportedTimezones } from "../general/timezone-settings";
import { requestCountDays, useProviderRequestCounts } from "./use-provider-request-counts";

export function ProviderTodayRequests({ count }: { count: number | undefined }) {
  const { t } = useTranslation();
  const formatting = useNumberFormatting();
  return (
    <span className="text-muted-foreground text-xs">
      {t("settings:provider_request_counts.today", {
        value:
          count === undefined ? "—" : formatting.formatDecimal(count, { maximumFractionDigits: 0 }),
      })}
    </span>
  );
}

interface ProviderRequestCountsPanelProps {
  query: ReturnType<typeof useProviderRequestCounts>;
  days: number;
  onDaysChange: (days: number) => void;
  providers: { id: string; name: string }[];
}

export function ProviderRequestCountsPanel({
  query,
  days,
  onDaysChange,
  providers,
}: ProviderRequestCountsPanelProps) {
  const { t } = useTranslation();
  const dates = useDateFormatting();
  const numbers = useNumberFormatting();
  const { settings, updateSettings } = useSettingsContext();
  const [selectedProvider, setSelectedProvider] = useState<string>();
  const [draftTimezone, setDraftTimezone] = useState<string>();
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState(false);
  const timezone = settings?.providerRequestTimezone ?? "Asia/Shanghai";
  const timezoneOptions = useMemo(
    () => Array.from(new Set(["Asia/Shanghai", ...getSupportedTimezones()])),
    [],
  );
  const choices = useMemo(() => {
    const names = new Map(providers.map((provider) => [provider.id, provider.name]));
    for (const row of query.data?.rows ?? [])
      if (!names.has(row.providerId)) names.set(row.providerId, row.providerId);
    return Array.from(names, ([value, label]) => ({ value, label }));
  }, [providers, query.data]);
  const providerId = choices.some((choice) => choice.value === selectedProvider)
    ? selectedProvider!
    : choices[0]?.value;
  // Never relabel old aggregates with a newly saved timezone while refetching.
  const data = query.data?.timezone === timezone ? query.data : undefined;
  const rows = data && providerId ? requestCountDays(data, providerId) : [];
  const rangeItems = [7, 30, 90].map((value) => ({
    value,
    label: t("settings:provider_request_counts.days", {
      value: numbers.formatDecimal(value, { maximumFractionDigits: 0 }),
    }),
  }));
  const saveTimezone = async () => {
    if (!draftTimezone) return;
    setSaving(true);
    setSaveError(false);
    try {
      await updateSettings({ providerRequestTimezone: draftTimezone });
      setDraftTimezone(undefined);
    } catch {
      setSaveError(true);
    } finally {
      setSaving(false);
    }
  };

  return (
    <section
      className="space-y-4 rounded-lg border p-4"
      aria-labelledby="provider-request-counts-title"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="space-y-1">
          <h2 id="provider-request-counts-title" className="font-medium">
            {t("settings:provider_request_counts.title")}
          </h2>
          <p className="text-muted-foreground text-sm">
            {t("settings:provider_request_counts.description")}
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          disabled={query.isFetching}
          onClick={() => void query.refetch()}
        >
          {t("settings:provider_request_counts.refresh")}
        </Button>
      </div>
      <div className="space-y-2">
        <Label>{t("settings:provider_request_counts.timezone")}</Label>
        <div className="flex flex-wrap items-center gap-2">
          <div className="w-full sm:w-72">
            <TimezoneInput
              value={draftTimezone ?? timezone}
              onChange={setDraftTimezone}
              timezones={timezoneOptions}
              ariaLabel={t("settings:provider_request_counts.timezone")}
              disabled={saving || !settings}
            />
          </div>
          <Button
            variant="outline"
            size="sm"
            disabled={saving || !settings || !draftTimezone || draftTimezone === timezone}
            onClick={() => void saveTimezone()}
          >
            {t(
              saving
                ? "settings:provider_request_counts.saving"
                : "settings:provider_request_counts.save",
            )}
          </Button>
        </div>
        <p className="text-muted-foreground text-xs">
          {t("settings:provider_request_counts.timezone_hint")}
        </p>
        {saveError && (
          <p role="alert" className="text-destructive text-sm">
            {t("settings:provider_request_counts.save_failed")}
          </p>
        )}
      </div>
      <div className="flex flex-wrap gap-3">
        <div className="min-w-0 flex-1 space-y-1">
          <Label htmlFor="request-count-provider">
            {t("settings:provider_request_counts.provider")}
          </Label>
          <Select
            items={choices}
            value={providerId ?? null}
            onValueChange={(value) => {
              if (value) setSelectedProvider(value);
            }}
            disabled={!choices.length}
          >
            <SelectTrigger id="request-count-provider" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {choices.map((choice) => (
                <SelectItem key={choice.value} value={choice.value}>
                  <bdi>{choice.label}</bdi>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-1">
          <Label htmlFor="request-count-range">{t("settings:provider_request_counts.range")}</Label>
          <Select
            items={rangeItems}
            value={days}
            onValueChange={(value) => {
              if (value !== null) onDaysChange(value);
            }}
          >
            <SelectTrigger id="request-count-range">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {rangeItems.map((item) => (
                <SelectItem key={item.value} value={item.value}>
                  {item.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>
      {query.isError && (
        <p role="alert" className="text-destructive text-sm">
          {t(
            data
              ? "settings:provider_request_counts.stale"
              : "settings:provider_request_counts.unavailable",
          )}
        </p>
      )}
      {!data && !query.isError && (
        <p role="status" className="text-muted-foreground text-sm">
          {t("settings:provider_request_counts.loading")}
        </p>
      )}
      {data && (
        <>
          <p className="text-muted-foreground text-xs">
            {t("settings:provider_request_counts.tracking_since", {
              value: dates.formatDate(data.trackingSince, {
                dateStyle: "medium",
                timeStyle: "short",
                timeZone: data.timezone,
              }),
            })}{" "}
            <bdi>({data.timezone})</bdi>
          </p>
          {!providerId ? (
            <p className="text-muted-foreground text-sm">
              {t("settings:provider_request_counts.no_providers")}
            </p>
          ) : (
            <div className="max-h-96 overflow-auto">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t("settings:provider_request_counts.date")}</TableHead>
                    <TableHead className="text-end">
                      {t("settings:provider_request_counts.requests")}
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {rows.map((row) => (
                    <TableRow key={row.day}>
                      <TableCell>
                        {dates.formatCalendarDate(row.day)}
                        {row.partial && (
                          <span className="text-muted-foreground ms-2 text-xs">
                            {t("settings:provider_request_counts.partial")}
                          </span>
                        )}
                      </TableCell>
                      <TableCell className="text-end tabular-nums">
                        {row.count === null
                          ? t("settings:provider_request_counts.not_tracked")
                          : numbers.formatDecimal(row.count, { maximumFractionDigits: 0 })}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
          <p className="text-muted-foreground text-xs">
            {t("settings:provider_request_counts.updated", {
              value: dates.formatDate(data.asOf, {
                dateStyle: "medium",
                timeStyle: "short",
                timeZone: data.timezone,
              }),
            })}{" "}
            <bdi>({data.timezone})</bdi>
          </p>
        </>
      )}
    </section>
  );
}
