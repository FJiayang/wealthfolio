import { Button } from "../ui/button";
import { Icons } from "../ui/icons";
import { motion } from "motion/react";
import { Spinner } from "../ui/spinner";

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

export interface StepNavigationProps {
  onNext: () => void;
  onBack: () => void;
  canGoBack: boolean;
  canGoNext: boolean;
  nextLabel: string;
  nextLoadingLabel: string;
  backLabel: string;
  isNextLoading?: boolean;
  showNext?: boolean;
}

// ─────────────────────────────────────────────────────────────────────────────
// Animation Variants
// ─────────────────────────────────────────────────────────────────────────────

const buttonVariants = {
  initial: { scale: 1 },
  hover: { scale: 1.02 },
  tap: { scale: 0.98 },
} as const;

// ─────────────────────────────────────────────────────────────────────────────
// Main Component
// ─────────────────────────────────────────────────────────────────────────────

export function StepNavigation({
  onNext,
  onBack,
  canGoBack,
  canGoNext,
  nextLabel,
  nextLoadingLabel,
  backLabel,
  isNextLoading = false,
  showNext = true,
}: StepNavigationProps) {
  return (
    <div className="flex flex-col-reverse justify-between gap-3 border-t pt-4 sm:flex-row">
      {/* Back button */}
      <motion.div
        whileHover={canGoBack ? "hover" : undefined}
        whileTap={canGoBack ? "tap" : undefined}
        variants={buttonVariants}
        className="w-full sm:w-auto"
      >
        <Button variant="outline" onClick={onBack} disabled={!canGoBack} className="w-full sm:w-auto">
          <Icons.ArrowLeft className="me-2 h-4 w-4 rtl:rotate-180" />
          {backLabel}
        </Button>
      </motion.div>

      {/* Next button */}
      {showNext && (
        <motion.div
          whileHover={canGoNext && !isNextLoading ? "hover" : undefined}
          whileTap={canGoNext && !isNextLoading ? "tap" : undefined}
          variants={buttonVariants}
          className="w-full sm:w-auto"
        >
          <Button onClick={onNext} disabled={!canGoNext || isNextLoading} className="w-full sm:w-auto">
            {isNextLoading ? (
              <>
                <Spinner className="me-2 h-4 w-4" />
                {nextLoadingLabel}
              </>
            ) : (
              <>
                {nextLabel}
                <Icons.ArrowRight className="ms-2 h-4 w-4 rtl:rotate-180" />
              </>
            )}
          </Button>
        </motion.div>
      )}
    </div>
  );
}

export default StepNavigation;
