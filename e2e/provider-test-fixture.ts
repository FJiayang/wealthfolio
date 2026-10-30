import { test as base, type APIRequestContext, type Page } from "@playwright/test";

// Request error messages can contain credential payloads. Keep operation and
// source frames (including nested cleanup failures), never raw messages/bodies.
function cleanupDiagnostic(error: unknown, depth = 0): object {
  if (!(error instanceof Error)) return { kind: "Non-Error cleanup failure" };
  const operation = error.message.match(
    /\b(?:apiRequestContext\.(?:get|put|post|delete)|toBeTruthy|toBe)\b/,
  )?.[0];
  const frames = (error.stack ?? "")
    .split("\n")
    .filter((line) => /^\s*at /.test(line))
    .map((line) => line.match(/(?:e2e|node_modules)\/[^\s()]+:\d+:\d+/)?.[0])
    .filter((frame): frame is string => Boolean(frame));
  return {
    kind: error instanceof AggregateError ? "AggregateError" : "Cleanup error",
    operation,
    frames,
    ...(error instanceof AggregateError && depth < 5
      ? {
          causes: Array.from(error.errors, (cause: unknown) => cleanupDiagnostic(cause, depth + 1)),
        }
      : {}),
  };
}

/** Provider tests mutate shared settings; cleanup needs a budget after body timeout. */
export const test = base.extend<{
  providerCleanup: (restore: (request: APIRequestContext) => Promise<void>) => void;
}>({
  providerCleanup: [
    async ({ context, playwright }, use, testInfo) => {
      const restorations: ((request: APIRequestContext) => Promise<void>)[] = [];
      await use((restore) => restorations.push(restore));
      const errors: unknown[] = [];
      let request: APIRequestContext | undefined;
      try {
        // Keep the test's profile/auth cookies, but not its expired page request context.
        request = await playwright.request.newContext({
          storageState: await context.storageState(),
          timeout: 5_000,
        });
        for (const restore of restorations.reverse()) {
          try {
            await restore(request);
          } catch (error) {
            errors.push(error);
          }
        }
      } catch (error) {
        errors.push(error);
      } finally {
        try {
          await request?.dispose();
        } catch (error) {
          errors.push(error);
        }
      }
      if (errors.length) {
        // Keep a failed body's original error; cleanup must still be visible.
        testInfo.annotations.push({
          type: "provider-cleanup-failure",
          description: `${errors.length} provider cleanup operation(s) failed`,
        });
        await testInfo.attach("provider-cleanup-errors", {
          body: JSON.stringify(
            errors.map((error) => cleanupDiagnostic(error)),
            null,
            2,
          ),
          contentType: "application/json",
        });
        if (testInfo.status === "passed") {
          throw new AggregateError(errors, "Provider fixture cleanup failed");
        }
      }
    },
    { timeout: 60_000 },
  ],
});

/** Disabled providers now live in the upstream catalog, not on the main list. */
export async function addProviderFromCatalog(page: Page, name: string) {
  await page.getByRole("button", { name: "Add provider", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Add a provider", exact: true })
    .getByRole("button", { name: `Add ${name}`, exact: true })
    .click();
}
