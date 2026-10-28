import { logger } from "@/adapters";

export function rawSyncErrorMessage(error: unknown): string {
  if (!error) return "";
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (typeof error === "number" || typeof error === "boolean" || typeof error === "bigint") {
    return String(error);
  }
  if (typeof error === "symbol") return error.description ?? "Unknown error";

  try {
    return JSON.stringify(error);
  } catch {
    return "Unknown error";
  }
}

function portfolioRepairMessage(message: string): string | null {
  const lower = message.toLowerCase();
  if (!lower.includes("deleted account link") || !lower.includes("settings > portfolios")) {
    return null;
  }

  const portfolioName = /Portfolio "([^"]+)"/.exec(message)?.[1];
  if (portfolioName) {
    return `Portfolio "${portfolioName}" contains a deleted account link. Open Settings > Portfolios, edit the portfolio, then save.`;
  }

  return "A portfolio contains a deleted account link. Open Settings > Portfolios, edit the portfolio, then save.";
}

function isTechnicalSyncError(message: string): boolean {
  const lower = message.toLowerCase();
  return (
    lower.includes("database operation") ||
    lower.includes("internal database") ||
    lower.includes("foreign key") ||
    lower.includes("constraint") ||
    lower.includes("sqlite") ||
    lower.includes("rowid=") ||
    lower.includes("table=") ||
    lower.includes("entity=") ||
    lower.includes(" pk=") ||
    lower.includes("cannot upload snapshot") ||
    lower.includes("snapshot bootstrap failed") ||
    lower.includes("snapshot restore failed")
  );
}

export function userFacingSyncErrorMessage(
  error: unknown,
  fallback = "Something went wrong. Please try again.",
): string {
  const message = rawSyncErrorMessage(error).trim();
  if (!message) return fallback;

  const lower = message.toLowerCase();
  const repairMessage = portfolioRepairMessage(message);
  if (repairMessage) return repairMessage;

  if (lower.includes("sync_source_restore_required")) {
    return "Sync needs to be restored from this device before you can connect another device.";
  }
  if (lower.includes("invalid") && lower.includes("code")) {
    return "Invalid code. Check and try again.";
  }
  if (lower.includes("expired")) return "Session expired. Please start again.";
  if (lower.includes("cancel")) return "Pairing was canceled.";
  if (lower.includes("network") || lower.includes("fetch")) {
    return "Network error. Check your connection.";
  }
  if (lower.includes("timeout")) return "Connection timed out.";
  if (lower.includes("not found")) return "Session not found or expired.";
  if (lower.includes("decrypt") || lower.includes("authentication")) {
    return "Security verification failed.";
  }

  if (isTechnicalSyncError(message)) {
    return "Sync could not finish. Please try again. If this keeps happening, check the app logs.";
  }

  return fallback;
}

export function logSyncError(context: string, error: unknown): void {
  const message = rawSyncErrorMessage(error);
  logger.error(`[DeviceSync] ${context}: ${message || "Unknown error"}`);
}
