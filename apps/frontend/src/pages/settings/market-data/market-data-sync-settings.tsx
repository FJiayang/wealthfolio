import { useState } from "react";
import { useTranslation } from "react-i18next";
import { usePlatform } from "@/hooks/use-platform";
import { useSettingsContext } from "@/lib/settings-provider";
import { Label } from "@wealthfolio/ui/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@wealthfolio/ui/components/ui/select";

export function MarketDataSyncSettings() {
  const { t } = useTranslation();
  const { settings, updateSettings } = useSettingsContext();
  const { isMobile, isWeb, loading } = usePlatform();
  const [saving, setSaving] = useState(false);

  if (!settings || loading || (isMobile && !isWeb)) return null;

  const items = [1, 6, 12, 24].map((hours) => ({
    value: hours,
    label: t("settings:market_data_page.sync_interval_hours", { count: hours }),
  }));

  const save = async (hours: number) => {
    setSaving(true);
    try {
      await updateSettings({ marketDataSyncIntervalHours: hours });
    } catch {
      // The settings mutation already reports the failure.
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex flex-col gap-3 rounded-lg border p-4 sm:flex-row sm:items-center sm:justify-between">
      <div className="space-y-1">
        <Label htmlFor="market-data-sync-interval">
          {t("settings:market_data_page.sync_interval_label")}
        </Label>
        <p id="market-data-sync-interval-description" className="text-muted-foreground text-sm">
          {t("settings:market_data_page.sync_interval_description")}
        </p>
      </div>
      <Select
        items={items}
        value={settings.marketDataSyncIntervalHours}
        onValueChange={(hours) => {
          if (hours !== null) void save(hours);
        }}
        disabled={saving}
      >
        <SelectTrigger
          id="market-data-sync-interval"
          aria-describedby="market-data-sync-interval-description"
          className="w-full sm:w-44 sm:shrink-0"
        >
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
}
