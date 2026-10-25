import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { usePlatform } from "@/hooks/use-platform";
import { useSettingsContext } from "@/lib/settings-provider";
import { Button } from "@wealthfolio/ui/components/ui/button";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { Label } from "@wealthfolio/ui/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@wealthfolio/ui/components/ui/select";
import { TimezoneInput } from "../general/timezone-input";
import { detectBrowserTimezone, getSupportedTimezones } from "../general/timezone-settings";

export function MarketDataSyncSettings() {
  const { t } = useTranslation();
  const { settings, updateSettings } = useSettingsContext();
  const { isMobile, isWeb, loading } = usePlatform();
  const [saving, setSaving] = useState(false);
  const browserTimezone = useMemo(() => detectBrowserTimezone(), []);
  const [mode, setMode] = useState<"interval" | "daily">("interval");
  const [hours, setHours] = useState(6);
  const [time, setTime] = useState("");
  const [timezone, setTimezone] = useState(browserTimezone);
  const timezones = useMemo(
    () =>
      Array.from(
        new Set([
          browserTimezone,
          ...(settings?.marketDataSyncTimezone ? [settings.marketDataSyncTimezone] : []),
          ...getSupportedTimezones(),
        ]),
      ),
    [browserTimezone, settings?.marketDataSyncTimezone],
  );

  useEffect(() => {
    setMode(settings?.marketDataSyncMode ?? "interval");
    setHours(settings?.marketDataSyncIntervalHours ?? 6);
    setTime(settings?.marketDataSyncTime ?? "");
    setTimezone(settings?.marketDataSyncTimezone ?? browserTimezone);
  }, [
    settings?.marketDataSyncMode,
    settings?.marketDataSyncIntervalHours,
    settings?.marketDataSyncTime,
    settings?.marketDataSyncTimezone,
    browserTimezone,
  ]);

  if (!settings || loading || (isMobile && !isWeb)) return null;

  const intervalItems = [1, 6, 12, 24].map((value) => ({
    value,
    label: t("settings:market_data_page.sync_interval_hours", { count: value }),
  }));
  const modeItems = [
    { value: "interval" as const, label: t("settings:market_data_page.sync_mode_interval") },
    { value: "daily" as const, label: t("settings:market_data_page.sync_mode_daily") },
  ];
  const validDaily = /^([01]\d|2[0-3]):[0-5]\d$/.test(time) && timezones.includes(timezone);
  const changed =
    mode !== (settings.marketDataSyncMode ?? "interval") ||
    hours !== settings.marketDataSyncIntervalHours ||
    time !== (settings.marketDataSyncTime ?? "") ||
    timezone !== (settings.marketDataSyncTimezone ?? browserTimezone);

  const save = async () => {
    if (mode === "daily" && !validDaily) return;
    setSaving(true);
    try {
      await updateSettings({
        marketDataSyncMode: mode,
        marketDataSyncIntervalHours: hours,
        // Save the daily pair together, including when switching back to interval.
        // An unfinished daily draft must not overwrite the previously saved pair.
        ...(validDaily ? { marketDataSyncTime: time, marketDataSyncTimezone: timezone } : {}),
      });
    } catch {
      // The settings mutation already reports the failure. Keep the draft for retry.
    } finally {
      setSaving(false);
    }
  };

  return (
    <div data-testid="market-data-sync-settings" className="space-y-4 rounded-lg border p-4">
      <p id="market-data-sync-description" className="text-muted-foreground text-sm">
        {t("settings:market_data_page.sync_schedule_description")}
      </p>
      <div className="grid gap-4 sm:grid-cols-2">
        <div className="space-y-2">
          <Label htmlFor="market-data-sync-mode">
            {t("settings:market_data_page.sync_mode_label")}
          </Label>
          <Select
            items={modeItems}
            value={mode}
            onValueChange={(value) => {
              if (value) setMode(value);
            }}
            disabled={saving}
          >
            <SelectTrigger
              id="market-data-sync-mode"
              aria-describedby="market-data-sync-description"
              className="w-full"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {modeItems.map((item) => (
                <SelectItem key={item.value} value={item.value}>
                  {item.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        {mode === "interval" ? (
          <div className="space-y-2">
            <Label htmlFor="market-data-sync-interval">
              {t("settings:market_data_page.sync_interval_label")}
            </Label>
            <Select
              items={intervalItems}
              value={hours}
              onValueChange={(value) => {
                if (value !== null) setHours(value);
              }}
              disabled={saving}
            >
              <SelectTrigger id="market-data-sync-interval" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {intervalItems.map((item) => (
                  <SelectItem key={item.value} value={item.value}>
                    {item.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        ) : (
          <>
            <div className="space-y-2">
              <Label htmlFor="market-data-sync-time">
                {t("settings:market_data_page.sync_daily_time_label")}
              </Label>
              <Input
                id="market-data-sync-time"
                type="time"
                step={60}
                value={time}
                onChange={(event) => setTime(event.target.value)}
                disabled={saving}
                dir="ltr"
                required
                aria-describedby="market-data-sync-daily-help"
              />
            </div>
            <div className="space-y-2 sm:col-span-2">
              <Label>{t("settings:market_data_page.sync_timezone_label")}</Label>
              <TimezoneInput
                value={timezone}
                onChange={setTimezone}
                timezones={timezones}
                disabled={saving}
                ariaLabel={t("settings:market_data_page.sync_timezone_label")}
              />
              <p id="market-data-sync-daily-help" className="text-muted-foreground text-sm">
                {t("settings:market_data_page.sync_daily_required")}
              </p>
            </div>
          </>
        )}
      </div>
      <div className="flex justify-end">
        <Button
          onClick={() => void save()}
          disabled={saving || !changed || (mode === "daily" && !validDaily)}
        >
          {t("common:save")}
        </Button>
      </div>
    </div>
  );
}
