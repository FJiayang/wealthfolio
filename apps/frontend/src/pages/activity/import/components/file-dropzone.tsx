import type { ComponentProps } from "react";
import { useTranslation } from "react-i18next";
import { FileDropzone as SharedFileDropzone } from "@wealthfolio/ui";

type Props = Omit<ComponentProps<typeof SharedFileDropzone>, "labels">;
export function FileDropzone(props: Props) {
  const { t } = useTranslation();
  return (
    <SharedFileDropzone
      {...props}
      labels={{
        remove: t("activity:import.upload.removeFile"),
        processing: t("activity:import.upload.processingFile"),
        choose: t("activity:import.upload.clickToUpload"),
        drop: t("activity:import.upload.orDrop"),
        hint: t("activity:import.upload.csvOnly"),
      }}
    />
  );
}
