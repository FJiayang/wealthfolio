import type { ComponentProps } from "react";
import { useTranslation } from "react-i18next";
import { CancelConfirmationDialog as SharedCancelConfirmationDialog } from "@wealthfolio/ui";

export type CancelConfirmationDialogProps = Omit<
  ComponentProps<typeof SharedCancelConfirmationDialog>,
  "title" | "description" | "continueLabel" | "cancelLabel"
> & { title?: string; description?: string };
export function CancelConfirmationDialog(props: CancelConfirmationDialogProps) {
  const { t } = useTranslation();
  return (
    <SharedCancelConfirmationDialog
      {...props}
      title={props.title ?? t("activity:import.dialog.cancelTitle")}
      description={props.description ?? t("activity:import.dialog.cancelDescription")}
      continueLabel={t("activity:import.dialog.continueImporting")}
      cancelLabel={t("activity:import.dialog.cancelImport")}
    />
  );
}
