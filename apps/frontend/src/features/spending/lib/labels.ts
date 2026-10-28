import type { TFunction } from "i18next";
import english from "@/i18n/locales/en/common.json";

/** Seeded budget groups are editable too; never translate a user rename. */
export function getBudgetGroupLabel(t: TFunction, id: string, name: string): string {
  const names: Record<string, string> = english.budgetGroupNames;
  return names[id] === name ? t(`common:budgetGroupNames.${id}`, { defaultValue: name }) : name;
}

/** Translate only unchanged seeded event types, never user-created types or event names. */
export function getEventTypeLabel(t: TFunction, id: string, name: string): string {
  const names: Record<string, string> = english.eventTypeNames;
  return names[id] === name ? t(`common:eventTypeNames.${id}`, { defaultValue: name }) : name;
}
