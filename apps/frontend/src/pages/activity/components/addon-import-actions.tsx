import { ActionPalette, type ActionPaletteItem } from "@/components/action-palette";
import { Button, Icons, buttonVariants } from "@wealthfolio/ui";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useNavigate } from "react-router-dom";

export function ActivityImportButton({ items }: { items: ActionPaletteItem[] }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);

  if (items.length === 0) {
    return (
      <Link
        to="/import"
        title={t("common:import")}
        className={buttonVariants({ variant: "outline", size: "icon" })}
      >
        <Icons.Import className="size-4" />
      </Link>
    );
  }

  return (
    <ActionPalette
      open={open}
      onOpenChange={setOpen}
      groups={[
        {
          items: [
            {
              icon: Icons.UploadSimple,
              label: t("activity:page.import_from_csv"),
              onClick: () => navigate("/import"),
            },
          ],
        },
        { items },
      ]}
      trigger={
        <Button variant="outline" size="icon" aria-label={t("common:import")}>
          <Icons.Import className="size-4" />
        </Button>
      }
    />
  );
}
