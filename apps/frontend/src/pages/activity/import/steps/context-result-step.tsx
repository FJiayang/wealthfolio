import { ImportResultSummary, useNumberFormatting } from "@wealthfolio/ui";
import { getImportHandoff, clearImportHandoff } from "@/addons/import-handoff";
import { Link, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { motion } from "motion/react";
import { Button } from "@wealthfolio/ui/components/ui/button";
import { Icons } from "@wealthfolio/ui/components/ui/icons";
import { useQuery } from "@tanstack/react-query";
import { getAccounts } from "@/adapters";
import { useShowConnectAccountLink } from "@/features/wealthfolio-connect/hooks/use-show-connect-account-link";
import { connectPagePath } from "@/features/wealthfolio-connect/lib/connect-url";
import { AccountType, isLiabilityAccountType } from "@/lib/constants";
import { QueryKeys } from "@/lib/query-keys";
import type { Account } from "@/lib/types";
import { useImportContext, reset, setStep } from "../context";
import { ImportAlert } from "../components/import-alert";
import { Spinner } from "@wealthfolio/ui";

// ─────────────────────────────────────────────────────────────────────────────
// Animation Variants
// ─────────────────────────────────────────────────────────────────────────────

const containerVariants = {
  hidden: { opacity: 0 },
  visible: {
    opacity: 1,
    transition: {
      when: "beforeChildren",
      staggerChildren: 0.15,
    },
  },
} as const;

const itemVariants = {
  hidden: { opacity: 0, y: 20 },
  visible: {
    opacity: 1,
    y: 0,
    transition: {
      duration: 0.5,
      ease: [0.22, 1, 0.36, 1] as [number, number, number, number],
    },
  },
} as const;

// ─────────────────────────────────────────────────────────────────────────────
// Main Component
// ─────────────────────────────────────────────────────────────────────────────

export function ContextResultStep() {
  const { state, dispatch } = useImportContext();
  const navigate = useNavigate();
  const { t } = useTranslation();
  const { formatDecimal } = useNumberFormatting();

  const { importResult, accountId } = state;

  const { data: accounts } = useQuery<Account[], Error>({
    queryKey: [QueryKeys.ACCOUNTS],
    queryFn: () => getAccounts(),
  });

  const account = accounts?.find((a) => a.id === accountId);
  const isHoldingsMode = account?.trackingMode === "HOLDINGS";

  // Broker sync is pitched for investment accounts and creates its own, so bank,
  // credit-card and already-synced imports get no link.
  const canShowConnectAccountLink = useShowConnectAccountLink();
  const showConnectAccountLink =
    canShowConnectAccountLink &&
    !!account &&
    !account.providerAccountId &&
    account.accountType !== AccountType.CASH &&
    !isLiabilityAccountType(account.accountType);

  const handleViewResult = () => {
    if (isHoldingsMode) {
      navigate(accountId ? `/holdings?account=${accountId}` : "/holdings");
    } else {
      navigate(accountId ? `/activities?account=${accountId}` : "/activities");
    }
  };

  const handleImportAnother = () => {
    const handoff = getImportHandoff();
    if (handoff) {
      clearImportHandoff();
      void navigate(handoff.returnRoute, { replace: true });
    } else dispatch(reset());
  };

  // Handle case where there's no import result
  if (!importResult) {
    return (
      <div className="flex flex-col items-center justify-center py-12">
        <Spinner className="text-primary h-8 w-8" />
        <p className="text-muted-foreground mt-4">{t("activity:import.result.loading")}</p>
      </div>
    );
  }

  const { success, stats, errorMessage } = importResult;

  if (!success) {
    return (
      <motion.div
        className="space-y-6"
        variants={containerVariants}
        initial="hidden"
        animate="visible"
      >
        <ImportAlert
          variant="destructive"
          title={t("activity:import.result.failedTitle")}
          description={errorMessage ?? t("activity:import.result.failedDescription")}
        />

        <motion.div className="flex justify-center gap-3" variants={itemVariants}>
          <Button variant="outline" onClick={() => dispatch(setStep("confirm"))}>
            <Icons.ArrowLeft className="me-2 h-4 w-4 rtl:rotate-180" />
            {t("activity:import.result.tryAgain")}
          </Button>
        </motion.div>
      </motion.div>
    );
  }

  return (
    <ImportResultSummary
      title={t("activity:import.result.completeTitle")}
      description={
        isHoldingsMode
          ? t("activity:import.result.holdingsSuccess")
          : t("activity:import.result.activitiesSuccess")
      }
      statistics={[
        { label: t("activity:import.result.imported"), value: formatDecimal(stats.imported) },
        { label: t("activity:import.result.duplicates"), value: formatDecimal(stats.duplicates) },
        { label: t("activity:import.result.skipped"), value: formatDecimal(stats.skipped) },
        { label: t("activity:import.result.total"), value: formatDecimal(stats.total) },
      ]}
    >
      {/* Action buttons */}
      <motion.div
        className="flex w-full flex-col gap-3 sm:w-auto sm:flex-row"
        variants={itemVariants}
      >
        <Button variant="outline" size="lg" onClick={handleImportAnother}>
          <Icons.Import className="me-2 h-4 w-4" />
          {getImportHandoff()
            ? t("activity:import.toolbar.returnToSource")
            : t("activity:import.result.importAnother")}
        </Button>

        <Button size="lg" onClick={handleViewResult}>
          {isHoldingsMode
            ? t("activity:import.result.viewHoldings")
            : t("activity:import.result.viewActivities")}
          <Icons.ArrowRight className="ms-2 h-4 w-4 rtl:rotate-180" />
        </Button>
      </motion.div>

      {showConnectAccountLink && (
        <motion.p className="text-muted-foreground mt-6 text-sm" variants={itemVariants}>
          {t("activity:import.result.syncAutomatically")}{" "}
          <Link
            to={connectPagePath("import_result")}
            className="text-primary font-medium underline-offset-4 hover:underline"
          >
            {t("settings:accounts_connect_account")}
          </Link>
        </motion.p>
      )}
    </ImportResultSummary>
  );
}

export default ContextResultStep;
