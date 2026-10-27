import { useTranslation } from "react-i18next";
import { useNumberFormatting } from "@wealthfolio/ui";
import { Button } from "@wealthfolio/ui/components/ui/button";

export function ProviderTodayRequests({
  count,
  onClick,
}: {
  count: number | undefined;
  onClick?: () => void;
}) {
  const { t } = useTranslation();
  const formatting = useNumberFormatting();
  const label = t("settings:provider_request_counts.today", {
    value:
      count === undefined ? "—" : formatting.formatDecimal(count, { maximumFractionDigits: 0 }),
  });
  return onClick ? (
    <Button
      variant="link"
      size="sm"
      className="text-muted-foreground h-auto p-0 text-xs font-normal"
      onClick={onClick}
      disabled={count === undefined}
    >
      {label}
    </Button>
  ) : (
    <span className="text-muted-foreground text-xs">{label}</span>
  );
}
