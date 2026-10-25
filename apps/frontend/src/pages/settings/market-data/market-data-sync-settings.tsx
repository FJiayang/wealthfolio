import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettingsContext } from "@/lib/settings-provider";
import { Label } from "@wealthfolio/ui/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@wealthfolio/ui/components/ui/select";
import { TimezoneInput } from "../general/timezone-input";
import { getSupportedTimezones } from "../general/timezone-settings";

export function MarketDataSyncSettings() {
  const { t } = useTranslation();
  const { settings, updateSettings } = useSettingsContext();
  const [saving, setSaving] = useState(false);
  const timezones = useMemo(
    () => Array.from(new Set(["Asia/Shanghai", ...getSupportedTimezones()])),
    [],
  );
  if (!settings) return null;
  const [hour, minute] = (settings.marketDataSyncTime ?? "22:00").split(":");
  const save = async (change: { marketDataSyncTime?: string; marketDataSyncTimezone?: string }) => {
    setSaving(true);
    try {
      await updateSettings(change);
    } catch {
      /* The settings mutation displays the save error. */
    } finally {
      setSaving(false);
    }
  };
  return (
    <div className="flex flex-col gap-3 rounded-lg border p-4">
      <Label>{t("settings:market_data_page.daily_sync_label")}</Label>
      <p className="text-muted-foreground text-sm">
        {t("settings:market_data_page.daily_sync_description")}
      </p>
      <div className="flex flex-wrap items-end gap-3">
        {(["hour", "minute"] as const).map((part) => {
          const items = Array.from({ length: part === "hour" ? 24 : 60 }, (_, i) => ({
            value: String(i).padStart(2, "0"),
            label: String(i).padStart(2, "0"),
          }));
          const label = t(`settings:market_data_page.sync_${part}`);
          return (
            <div key={part} className="flex flex-col gap-2">
              <Label htmlFor={`sync-${part}`}>{label}</Label>
              <Select
                items={items}
                value={part === "hour" ? hour : minute}
                disabled={saving}
                onValueChange={(value) => {
                  if (value !== null)
                    void save({
                      marketDataSyncTime:
                        part === "hour" ? `${value}:${minute}` : `${hour}:${value}`,
                    });
                }}
              >
                <SelectTrigger id={`sync-${part}`} className="w-24">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {items.map((item) => (
                    <SelectItem key={item.value} value={item.value}>
                      {item.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          );
        })}
        <div className="flex min-w-48 flex-col gap-2">
          <Label>{t("settings:provider_request_counts.timezone")}</Label>
          <TimezoneInput
            value={settings.marketDataSyncTimezone ?? "Asia/Shanghai"}
            timezones={timezones}
            disabled={saving}
            onChange={(timezone) => void save({ marketDataSyncTimezone: timezone })}
          />
        </div>
      </div>
    </div>
  );
}
