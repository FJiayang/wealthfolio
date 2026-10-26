import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@wealthfolio/ui/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@wealthfolio/ui/components/ui/sheet";
import { ActivityTypePicker } from "@/pages/activity/components/activity-type-picker";
import { InstructionForm } from "./instruction-form";

export function InstructionSheet({ goalId, onClose }: { goalId: string; onClose: () => void }) {
  const { t } = useTranslation("trade-instructions");
  const [side, setSide] = useState<"BUY" | "SELL">();
  return (
    <Sheet
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <SheetContent
        data-testid="instruction-form-dialog"
        className="flex w-full flex-col overflow-hidden sm:max-w-[39.0625rem]"
      >
        <SheetHeader>
          <SheetTitle>{t("create")}</SheetTitle>
          <SheetDescription>{t("independent")}</SheetDescription>
        </SheetHeader>
        <div className="flex-1 space-y-6 overflow-y-auto py-4">
          <ActivityTypePicker
            value={side}
            allowedTypes={["BUY", "SELL"]}
            onSelect={(type) => {
              if (type === "BUY" || type === "SELL") setSide(type);
            }}
          />
          {side ? (
            <InstructionForm
              key={side}
              goalId={goalId}
              side={side}
              onSaved={onClose}
              onCancel={onClose}
            />
          ) : (
            <div className="text-muted-foreground flex h-40 items-center justify-center">
              {t("selectType")}
            </div>
          )}
        </div>
        {!side && (
          <SheetFooter>
            <Button variant="outline" onClick={onClose}>
              {t("common:cancel")}
            </Button>
          </SheetFooter>
        )}
      </SheetContent>
    </Sheet>
  );
}
