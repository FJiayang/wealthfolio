import { Badge } from "@wealthfolio/ui";
import { useTranslation } from "react-i18next";
import type { InstructionSide } from "@/adapters";
import { ActivityTypeBadge } from "@/pages/activity/components/activity-type-badge";

export function InstructionTypeBadge({ side }: { side: InstructionSide }) {
  const { t } = useTranslation("trade-instructions");
  return side === "TRANSFER" || side === "CONVERSION" ? (
    <Badge variant="secondary">{t(side)}</Badge>
  ) : (
    <ActivityTypeBadge type={side} />
  );
}
