import { WizardStepIndicator as SharedWizardStepIndicator } from "@wealthfolio/ui";
import { useTranslation } from "react-i18next";
import type { ImportStep } from "../context";
export interface WizardStep {
  id: ImportStep;
  label: string;
}
export function WizardStepIndicator({
  steps,
  currentStep,
}: {
  steps: WizardStep[];
  currentStep: ImportStep;
}) {
  const { t } = useTranslation();
  return (
    <SharedWizardStepIndicator
      label={t("activity:import.page.titleActivities")}
      steps={steps}
      currentStep={currentStep}
    />
  );
}
