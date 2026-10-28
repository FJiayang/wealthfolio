import type { ComponentProps } from "react";
import { useTranslation } from "react-i18next";
import { StepNavigation as SharedStepNavigation } from "@wealthfolio/ui";

export type StepNavigationProps = Omit<
  ComponentProps<typeof SharedStepNavigation>,
  "nextLabel" | "nextLoadingLabel" | "backLabel"
> & { nextLabel?: string; nextLoadingLabel?: string; backLabel?: string };
export function StepNavigation(props: StepNavigationProps) {
  const { t } = useTranslation();
  return (
    <SharedStepNavigation
      {...props}
      nextLabel={props.nextLabel ?? t("activity:import.toolbar.continue")}
      nextLoadingLabel={props.nextLoadingLabel ?? t("activity:import.toolbar.processing")}
      backLabel={props.backLabel ?? t("activity:import.toolbar.back")}
    />
  );
}
