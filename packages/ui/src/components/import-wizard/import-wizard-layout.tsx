import type { ReactNode } from "react";
import { motion } from "motion/react";
import { ErrorBoundary } from "../common/error-boundary";
import { Icons } from "../ui/icons";
import { Card, CardHeader } from "../ui/card";

export interface ImportWizardLayoutProps {
  children: ReactNode;
  steps?: ReactNode;
  navigation?: ReactNode;
  isResult?: boolean;
}

export function ImportWizardLayout({ children, steps, navigation, isResult }: ImportWizardLayoutProps) {
  return (
    <ErrorBoundary variant="page">
      <Card data-import-wizard className="flex h-full min-h-0 w-full flex-col gap-0 overflow-hidden">
        {!isResult && steps && <CardHeader className="shrink-0 border-b px-3 py-3 sm:px-6 sm:py-4">{steps}</CardHeader>}
        <div className="min-h-0 flex-1 overflow-y-auto p-3 sm:p-6">{children}</div>
        {navigation && <div className="shrink-0 px-3 pb-3 sm:px-6 sm:pb-6">{navigation}</div>}
      </Card>
    </ErrorBoundary>
  );
}

export interface ImportResultSummaryProps {
  title: string;
  description?: ReactNode;
  outcome?: "success" | "partial" | "uncertain";
  statistics: { label: string; value: ReactNode }[];
  children?: ReactNode;
}

export function ImportResultSummary({
  title,
  description,
  outcome = "success",
  statistics,
  children,
}: ImportResultSummaryProps) {
  return (
    <div className="flex flex-col items-center gap-6 py-6 text-center" role="status">
      <motion.div
        initial={{ scale: 0 }}
        animate={{ scale: 1 }}
        className={
          outcome === "success"
            ? "bg-success-subtle text-success-subtle-foreground rounded-full"
            : "bg-warning-subtle text-warning-subtle-foreground rounded-full"
        }
      >
        <span className="flex size-16 items-center justify-center" aria-hidden="true">
          {outcome === "success" ? (
            <Icons.CheckCircle className="size-9" />
          ) : (
            <Icons.AlertTriangle className="size-9" />
          )}
        </span>
      </motion.div>
      <div>
        <h2 className="text-xl font-semibold">{title}</h2>
        {description && <div className="text-muted-foreground mt-2 text-sm">{description}</div>}
      </div>
      <dl className="flex flex-wrap justify-center gap-8">
        {statistics.map(({ label, value }) => (
          <div key={label}>
            <dt className="text-muted-foreground text-sm">{label}</dt>
            <dd className="text-2xl font-semibold tabular-nums">{value}</dd>
          </div>
        ))}
      </dl>
      <div className="flex w-full flex-col gap-3 sm:w-auto sm:flex-row">{children}</div>
    </div>
  );
}
