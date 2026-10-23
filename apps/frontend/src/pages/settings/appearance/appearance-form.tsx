import { useState } from "react";
import { zodResolver } from "@hookform/resolvers/zod";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import * as z from "zod";

import { FontSelector } from "@/components/font-selector";
import { NavigationStyleSelector } from "@/components/navigation-style-selector";
import { ThemeSelector } from "@/components/theme-selector";
import {
  Form,
  FormControl,
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@wealthfolio/ui/components/ui/form";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@wealthfolio/ui/components/ui/select";
import type { GainLossColorScheme } from "@/lib/types";
import { Switch } from "@wealthfolio/ui/components/ui/switch";
import { Item, ItemActions, ItemContent, ItemTitle } from "@wealthfolio/ui/components/ui/item";
import { usePlatform } from "@/hooks/use-platform";
import { useSettingsContext } from "@/lib/settings-provider";
import { useNavigationMode } from "@/pages/layouts/navigation/navigation-mode-context";

interface AppearanceFormValues {
  theme: "light" | "dark" | "system";
  font: "font-mono" | "font-sans" | "font-serif";
  menuBarVisible: boolean;
  gainLossColorScheme: GainLossColorScheme;
}

export function AppearanceForm() {
  const { t } = useTranslation();
  const [savingColorScheme, setSavingColorScheme] = useState(false);
  const { settings, updateSettings } = useSettingsContext();
  const { isMobile, isMacOS, isTauri } = usePlatform();
  const { mode: navigationMode, setMode: setNavigationMode } = useNavigationMode();
  const appearanceFormSchema = z.object({
    theme: z.enum(["light", "dark", "system"], {
      required_error: t("settings:appearance_theme_required"),
    }),
    font: z.enum(["font-mono", "font-sans", "font-serif"], {
      invalid_type_error: t("settings:appearance_font_error"),
      required_error: t("settings:appearance_font_required"),
    }),
    menuBarVisible: z.boolean(),
    gainLossColorScheme: z.enum(["green-up-red-down", "red-up-green-down"]),
  });
  const defaultValues: Partial<AppearanceFormValues> = {
    theme: settings?.theme as AppearanceFormValues["theme"],
    font: settings?.font as AppearanceFormValues["font"],
    menuBarVisible: settings?.menuBarVisible ?? true,
    gainLossColorScheme: settings?.gainLossColorScheme ?? "green-up-red-down",
  };
  const form = useForm<AppearanceFormValues>({
    resolver: zodResolver(appearanceFormSchema),
    defaultValues,
  });

  function handlePartialUpdate(data: Partial<AppearanceFormValues>) {
    updateSettings(data).catch((error) => {
      console.error("Failed to update appearance settings:", error);
    });
  }

  return (
    <Form {...form}>
      <div className="max-w-4xl space-y-6">
        <FormField
          control={form.control}
          name="font"
          render={({ field }) => (
            <FormItem className="gap-3">
              <div className="space-y-1">
                <FormLabel className="text-base font-medium">
                  {t("settings:appearance_font_title")}
                </FormLabel>
                <FormDescription className="text-sm">
                  {t("settings:appearance_font_description")}
                </FormDescription>
              </div>
              <FormControl>
                <FontSelector
                  value={field.value}
                  onChange={(value) => {
                    field.onChange(value);
                    handlePartialUpdate({ font: value as AppearanceFormValues["font"] });
                  }}
                />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="theme"
          render={({ field }) => (
            <FormItem className="gap-3">
              <div className="space-y-1">
                <FormLabel className="text-base font-medium">{t("settings:theme")}</FormLabel>
                <FormDescription className="text-sm">
                  {t("settings:appearance_theme_description")}
                </FormDescription>
              </div>
              <FormMessage />
              <FormControl>
                <ThemeSelector
                  value={field.value}
                  onChange={(value) => {
                    field.onChange(value);
                    handlePartialUpdate({ theme: value as AppearanceFormValues["theme"] });
                  }}
                  className="pt-2"
                />
              </FormControl>
            </FormItem>
          )}
        />

        <FormField
          control={form.control}
          name="gainLossColorScheme"
          render={({ field }) => (
            <FormItem>
              <FormLabel>{t("settings:appearance_gain_loss_color_title")}</FormLabel>
              <FormDescription>
                {t("settings:appearance_gain_loss_color_description")}
              </FormDescription>
              <Select
                value={field.value}
                items={[
                  {
                    value: "green-up-red-down",
                    label: t("settings:appearance_gain_loss_green_up"),
                  },
                  { value: "red-up-green-down", label: t("settings:appearance_gain_loss_red_up") },
                ]}
                disabled={savingColorScheme}
                onValueChange={async (value) => {
                  if (value === null) return;
                  const previous = settings?.gainLossColorScheme ?? "green-up-red-down";
                  field.onChange(value);
                  setSavingColorScheme(true);
                  try {
                    await updateSettings({ gainLossColorScheme: value });
                  } catch {
                    field.onChange(previous);
                  } finally {
                    setSavingColorScheme(false);
                  }
                }}
              >
                <FormControl>
                  <SelectTrigger data-testid="gain-loss-color-select" className="w-full sm:w-72">
                    <SelectValue />
                  </SelectTrigger>
                </FormControl>
                <SelectContent>
                  <SelectItem value="green-up-red-down">
                    {t("settings:appearance_gain_loss_green_up")}
                  </SelectItem>
                  <SelectItem value="red-up-green-down">
                    {t("settings:appearance_gain_loss_red_up")}
                  </SelectItem>
                </SelectContent>
              </Select>
              <FormMessage />
            </FormItem>
          )}
        />

        {!isMobile && (
          <div className="space-y-3">
            <div className="space-y-1">
              <p className="text-base font-medium">{t("settings:appearance_navigation_title")}</p>
              <p className="text-muted-foreground text-sm">
                {t("settings:appearance_navigation_description")}
              </p>
            </div>
            <NavigationStyleSelector
              value={navigationMode}
              onChange={setNavigationMode}
              className="max-w-md"
            />
          </div>
        )}

        {!isMobile && !(isTauri && isMacOS) && (
          <FormField
            control={form.control}
            name="menuBarVisible"
            render={({ field }) => (
              <FormItem>
                <Item variant="outline" size="sm">
                  <ItemContent>
                    <ItemTitle>
                      <FormLabel>{t("settings:appearance_menu_bar")}</FormLabel>
                    </ItemTitle>
                    <FormDescription>
                      {t("settings:appearance_menu_bar_description")}
                    </FormDescription>
                  </ItemContent>
                  <ItemActions>
                    <FormControl>
                      <Switch
                        checked={field.value}
                        onCheckedChange={(value) => {
                          field.onChange(value);
                          handlePartialUpdate({ menuBarVisible: value });
                        }}
                      />
                    </FormControl>
                  </ItemActions>
                </Item>
              </FormItem>
            )}
          />
        )}
      </div>
    </Form>
  );
}
