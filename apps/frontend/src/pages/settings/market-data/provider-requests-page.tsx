import { useMemo, useState } from "react";
import { useInfiniteQuery } from "@tanstack/react-query";
import { today } from "@internationalized/date";
import { useNavigate, useSearchParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useDateFormatting, useNumberFormatting } from "@wealthfolio/ui";
import { Button } from "@wealthfolio/ui/components/ui/button";
import { Badge } from "@wealthfolio/ui/components/ui/badge";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { Label } from "@wealthfolio/ui/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@wealthfolio/ui/components/ui/select";
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetDescription,
} from "@wealthfolio/ui/components/ui/sheet";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@wealthfolio/ui/components/ui/table";
import { listProviderRequests } from "@/adapters";
import type { ProviderRequestQuery } from "@/adapters/shared/market-data";
import { selectedProfileId } from "@/features/profiles/session";
import { useCustomProviders } from "@/hooks/use-custom-providers";
import { useSettingsContext } from "@/lib/settings-provider";
import { getProviderName } from "@/lib/provider-labels";
import { DetailRow, DetailSection } from "@/pages/activity/components/activity-detail-sheet";
import { SettingsHeader } from "../settings-header";
import { TimezoneInput } from "../general/timezone-input";
import { getSupportedTimezones } from "../general/timezone-settings";
import { useMarketDataProviderSettings } from "./use-market-data-settings";
import { providerRequestDateBounds } from "./provider-request-dates";

export function ProviderRequestsPage() {
  const { t } = useTranslation();
  const label = (key: string) => t(`settings:provider_request_details.${key}`);
  const { formatDate } = useDateFormatting();
  const numbers = useNumberFormatting();
  const { settings, updateSettings } = useSettingsContext();
  const timezone = settings?.providerRequestTimezone ?? "Asia/Shanghai";
  const [draftTimezone, setDraftTimezone] = useState<string>();
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState(false);
  const timezoneOptions = useMemo(
    () => Array.from(new Set(["Asia/Shanghai", ...getSupportedTimezones()])),
    [],
  );
  const [params, setParams] = useSearchParams();
  const navigate = useNavigate();
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const { data: providers = [] } = useMarketDataProviderSettings();
  const { data: customProviders = [] } = useCustomProviders();
  const currentDay = today(timezone);
  const from = params.get("from") ?? currentDay.subtract({ days: 29 }).toString();
  const to = params.get("to") ?? currentDay.toString();
  const providerId = params.get("providerId") ?? "";
  const outcome = params.get("outcome") ?? "";
  const kind = params.get("recordKind") ?? "";
  const symbol = params.get("symbol") ?? "";
  const bounds = providerRequestDateBounds(from, to, timezone);
  const valid =
    !!bounds &&
    ["", "success", "failure", "unknown"].includes(outcome) &&
    ["", "http", "sdk"].includes(kind);
  const request: ProviderRequestQuery = {
    ...bounds,
    providerId: providerId || undefined,
    symbol: symbol || undefined,
    outcome: (outcome || undefined) as ProviderRequestQuery["outcome"],
    recordKind: (kind || undefined) as ProviderRequestQuery["recordKind"],
    limit: 50,
  };
  const query = useInfiniteQuery({
    queryKey: ["provider-request-details", selectedProfileId(), timezone, request],
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) => listProviderRequests({ ...request, cursor: pageParam }),
    getNextPageParam: (last) => last.nextCursor ?? undefined,
    enabled: valid,
  });
  const rows = query.data?.pages.flatMap((page) => page.rows) ?? [];
  const selected = rows.find((row) => row.id === selectedId);
  const names = new Map([
    ...providers
      .filter((provider) => provider.id !== "CUSTOM_SCRAPER")
      .map((provider) => [provider.id, getProviderName(t, provider.id, provider.name)] as const),
    ...customProviders.map((provider) => [`CUSTOM_SCRAPER:${provider.id}`, provider.name] as const),
  ]);
  if (providerId && !names.has(providerId)) names.set(providerId, providerId);
  const change = (key: string, value: string) => {
    setSelectedId(null);
    setParams(
      (previous) => {
        const next = new URLSearchParams(previous);
        if (value) next.set(key, value);
        else if (key === "from" || key === "to") next.set(key, "");
        else next.delete(key);
        return next;
      },
      { replace: true },
    );
  };
  const date = (value: string) =>
    formatDate(value, { dateStyle: "medium", timeStyle: "medium", timeZone: timezone });
  const duration = (value: number | undefined) =>
    value === undefined ? "—" : `${numbers.formatDecimal(value, { maximumFractionDigits: 0 })} ms`;
  const resultBadge = (value: string) => (
    <Badge variant={value === "failure" ? "destructive-subtle" : "secondary"}>{label(value)}</Badge>
  );
  const selectors = [
    {
      key: "providerId",
      title: t("settings:provider_request_counts.provider"),
      value: providerId,
      items: Array.from(names, ([value, label]) => ({ value, label })),
    },
    {
      key: "outcome",
      title: label("outcome"),
      value: outcome,
      items: ["success", "failure", "unknown"].map((value) => ({ value, label: label(value) })),
    },
    {
      key: "recordKind",
      title: label("kind"),
      value: kind,
      items: ["http", "sdk"].map((value) => ({ value, label: value.toUpperCase() })),
    },
  ];
  const saveTimezone = async () => {
    if (!draftTimezone) return;
    setSaving(true);
    setSaveError(false);
    try {
      await updateSettings({ providerRequestTimezone: draftTimezone });
      setDraftTimezone(undefined);
      setSelectedId(null);
    } catch {
      setSaveError(true);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-6">
      <SettingsHeader
        heading={label("title")}
        text={t("settings:provider_request_counts.retention_hint")}
        backTo="/settings/market-data"
      >
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            onClick={() =>
              navigate(
                `/settings/market-data${params.get("dir") ? `?dir=${params.get("dir")}` : ""}`,
              )
            }
          >
            {t("common:back")}
          </Button>
          <Button
            variant="outline"
            disabled={!valid || query.isFetching}
            onClick={() => void query.refetch()}
          >
            {t("settings:provider_request_counts.refresh")}
          </Button>
        </div>
      </SettingsHeader>
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
      <p className="text-muted-foreground text-sm">
        <bdi>{timezone}</bdi> · {label("timing")}
      </p>
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        {selectors.map((selector) => {
          const items = [{ value: "all", label: label("all") }, ...selector.items];
          return (
            <div key={selector.key} className="space-y-1">
              <Label htmlFor={`request-${selector.key}`}>{selector.title}</Label>
              <Select
                items={items}
                value={selector.value || "all"}
                onValueChange={(value) => {
                  if (value) change(selector.key, value === "all" ? "" : value);
                }}
              >
                <SelectTrigger id={`request-${selector.key}`} className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {items.map((item) => (
                    <SelectItem key={item.value} value={item.value}>
                      <bdi>{item.label}</bdi>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          );
        })}
        {[
          { key: "from", value: from },
          { key: "to", value: to },
        ].map((field) => (
          <div key={field.key} className="space-y-1">
            <Label htmlFor={`request-${field.key}`}>{label(field.key)}</Label>
            <Input
              id={`request-${field.key}`}
              type="date"
              value={field.value}
              min={currentDay.subtract({ days: 30 }).toString()}
              max={currentDay.toString()}
              onChange={(event) => change(field.key, event.target.value)}
            />
          </div>
        ))}
        <div className="space-y-1">
          <Label htmlFor="request-symbol">{label("symbol")}</Label>
          <Input
            id="request-symbol"
            value={symbol}
            onChange={(event) => change("symbol", event.target.value)}
          />
        </div>
      </div>
      {!valid && (
        <p role="alert" className="text-destructive text-sm">
          {label("invalid_range")}
        </p>
      )}
      {query.isError && (
        <p role="alert" className="text-destructive text-sm">
          {label("unavailable")}
        </p>
      )}
      {valid && query.isPending && <p role="status">{label("loading")}</p>}
      {valid && query.isSuccess && rows.length === 0 && (
        <p role="status" className="text-muted-foreground">
          {label("empty")}
        </p>
      )}
      {rows.length > 0 && (
        <div className="min-w-0 overflow-x-auto rounded-lg border">
          <Table>
            <TableHeader>
              <TableRow>
                {[
                  "time",
                  "provider",
                  "symbols",
                  "operation",
                  "kind",
                  "outcome",
                  "duration",
                  "http_status",
                  "error",
                  "view",
                ].map((key) => (
                  <TableHead
                    key={key}
                    className={
                      key === "view"
                        ? "bg-background sticky end-0 whitespace-nowrap"
                        : "whitespace-nowrap"
                    }
                  >
                    {key === "provider"
                      ? t("settings:provider_request_counts.provider")
                      : label(key)}
                  </TableHead>
                ))}
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((row) => (
                <TableRow key={row.id}>
                  <TableCell className="whitespace-nowrap">{date(row.occurredAt)}</TableCell>
                  <TableCell>
                    <bdi>{names.get(row.providerId) ?? row.providerId}</bdi>
                  </TableCell>
                  <TableCell>
                    <bdi>{row.context?.symbols.join(", ") || "—"}</bdi>
                  </TableCell>
                  <TableCell className="max-w-56 break-words">
                    {row.context?.operation ?? "—"}
                  </TableCell>
                  <TableCell>{(row.context?.recordKind ?? "http").toUpperCase()}</TableCell>
                  <TableCell>{resultBadge(row.result?.outcome ?? "unknown")}</TableCell>
                  <TableCell className="whitespace-nowrap tabular-nums">
                    {duration(row.result?.durationMs)}
                  </TableCell>
                  <TableCell>{row.result?.httpStatus ?? "—"}</TableCell>
                  <TableCell className="max-w-64 min-w-40 break-words">
                    {row.result?.errorMessage ?? "—"}
                  </TableCell>
                  <TableCell className="bg-background sticky end-0">
                    <Button
                      variant="ghost"
                      size="sm"
                      aria-label={`${label("view")} #${row.id}`}
                      onClick={() => setSelectedId(row.id)}
                    >
                      {label("view")}
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
      {query.hasNextPage && (
        <Button
          variant="outline"
          disabled={query.isFetchingNextPage}
          onClick={() => void query.fetchNextPage()}
        >
          {label(query.isFetchingNextPage ? "loading" : "more")}
        </Button>
      )}
      {query.data && (
        <p className="text-muted-foreground text-xs">
          {t("settings:provider_request_details.available_since", {
            value: date(query.data.pages[0].availableSince),
          })}{" "}
          {t("settings:provider_request_details.diagnostics_since", {
            value: date(query.data.pages[0].diagnosticsSince),
          })}
        </p>
      )}
      <Sheet
        open={!!selected}
        onOpenChange={(open) => {
          if (!open) setSelectedId(null);
        }}
      >
        <SheetContent side="inline-end" className="flex w-full flex-col sm:max-w-xl">
          <SheetHeader className="pe-10">
            <SheetTitle>{label("detail_title")}</SheetTitle>
            <SheetDescription>
              <bdi>{selected ? (names.get(selected.providerId) ?? selected.providerId) : ""}</bdi>
            </SheetDescription>
          </SheetHeader>
          {selected && (
            <div className="min-h-0 flex-1 space-y-6 overflow-y-auto">
              {!selected.context && (
                <p className="text-muted-foreground text-sm">{label("legacy")}</p>
              )}
              {!selected.result && (
                <p className="text-muted-foreground text-sm">{label("unfinished")}</p>
              )}
              <DetailSection title={label("detail_title")}>
                <DetailRow label={label("identifier")} value={selected.id} />
                <DetailRow
                  label={label("time")}
                  value={
                    <span className="break-all">
                      <span>{date(selected.occurredAt)}</span>
                      <br />
                      <code dir="ltr">{selected.occurredAt}</code>
                    </span>
                  }
                />
                <DetailRow
                  label={label("symbols")}
                  value={
                    <bdi className="break-all">{selected.context?.symbols.join(", ") || "—"}</bdi>
                  }
                />
                <DetailRow
                  label={label("operation")}
                  value={<bdi className="break-all">{selected.context?.operation ?? "—"}</bdi>}
                />
                <DetailRow
                  label={label("kind")}
                  value={(selected.context?.recordKind ?? "http").toUpperCase()}
                />
                <DetailRow
                  label={label("outcome")}
                  value={resultBadge(selected.result?.outcome ?? "unknown")}
                />
                <DetailRow
                  label={label("duration")}
                  value={duration(selected.result?.durationMs)}
                />
                <DetailRow
                  label={label("http_status")}
                  value={selected.result?.httpStatus ?? "—"}
                />
              </DetailSection>
              <DetailSection title={label("error")}>
                <DetailRow
                  label={label("error_category")}
                  value={selected.result?.errorCategory ?? "—"}
                />
                <p className="text-sm break-words">{selected.result?.errorMessage ?? "—"}</p>
              </DetailSection>
              <DetailSection title={label("parameters")}>
                {selected.context ? (
                  <pre
                    dir="ltr"
                    className="max-w-full text-start text-xs break-all whitespace-pre-wrap"
                  >
                    <code>{JSON.stringify(selected.context.parameters, null, 2)}</code>
                  </pre>
                ) : (
                  <p className="text-muted-foreground text-sm">{label("legacy")}</p>
                )}
              </DetailSection>
            </div>
          )}
        </SheetContent>
      </Sheet>
    </div>
  );
}
