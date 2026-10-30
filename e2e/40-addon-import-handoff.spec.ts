import { expect, test } from "@playwright/test";
import { BASE_URL, loginIfNeeded } from "./helpers";

test("parsed drafts enter the host asset, review, confirm and result flow", async ({ page }) => {
  test.setTimeout(180_000);
  await loginIfNeeded(page);
  await page.goto(BASE_URL + "/activities", { waitUntil: "domcontentloaded" });
  const requestedId = "addon-handoff-account";
  const response = await page.request.post(BASE_URL + "/api/v1/accounts", {
    data: {
      id: requestedId,
      name: "Addon Handoff Account",
      accountType: "SECURITIES",
      currency: "USD",
      isDefault: false,
      isActive: true,
      isArchived: false,
      trackingMode: "TRANSACTIONS",
    },
  });
  expect(response.ok()).toBe(true);
  const { id: accountId } = await response.json();

  const input = {
    sourceLabel: "synthetic statement",
    accountId,
    rows: [
      {
        sourceLine: 12,
        rawRow: ["synthetic", "BUY"],
        excluded: false,
        activity: {
          accountId,
          activityType: "BUY",
          symbol: "AAPL",
          currency: "USD",
          date: "2026-01-06T00:00:00Z",
          quantity: "2",
          unitPrice: "100",
          amount: "201",
          fee: "1",
          isDraft: true,
          isValid: false,
          forceImport: true,
          errors: { amount: ["must be discarded"] },
          comment: "synthetic addon handoff",
        },
      },
      {
        sourceLine: 13,
        rawRow: ["excluded"],
        excluded: true,
        activity: {
          accountId,
          activityType: "BUY",
          currency: "USD",
          isDraft: true,
          isValid: false,
        },
      },
    ],
  };

  async function open() {
    await page.evaluate(async (request) => {
      const modulePath = "/src/addons/import-handoff.ts";
      const { prepareImportHandoff } = await import(modulePath);
      prepareImportHandoff("synthetic-addon", request, "/activities");
      history.pushState(null, "", "/import?addonImport=1");
      dispatchEvent(new PopStateEvent("popstate"));
    }, input);
    await expect(page.getByRole("button", { name: /Review Activities/i })).toBeEnabled({
      timeout: 30_000,
    });
  }
  await open();
  await page.getByRole("button", { name: "Return to original transactions" }).click();
  await expect(page.getByRole("alertdialog")).toContainText("discards edits");
  await page.getByRole("button", { name: "Continue Importing" }).click();
  await expect(page.getByRole("button", { name: /Review Activities/i })).toBeEnabled();
  await page.getByRole("button", { name: "Return to original transactions" }).click();
  await page.getByRole("alertdialog").getByRole("button", { name: "Cancel Import" }).click();
  await expect(page).toHaveURL(/\/activities$/);

  await open();
  await page.getByRole("button", { name: /Review Activities/i }).click();
  await expect(page.getByRole("button", { name: /Continue to Import/i })).toBeEnabled({
    timeout: 30_000,
  });
  // The footer updates before the outgoing asset-review animation finishes.
  await expect(
    page.locator("[data-import-wizard]").getByRole("grid", { name: "Data grid", exact: true }),
  ).toBeVisible();
  await expect(page.getByText("must be discarded")).toHaveCount(0);
  const wizard = await page.locator("[data-import-wizard]").boundingBox();
  expect(wizard!.height).toBeGreaterThan(350);
  const nextButton = await page.getByRole("button", { name: /Continue to Import/i }).boundingBox();
  expect(nextButton!.y + nextButton!.height).toBeLessThanOrEqual(page.viewportSize()!.height);
  await page.getByRole("button", { name: /Continue to Import/i }).click();
  await expect(page.getByRole("button", { name: /Import 1 Activit/i })).toBeVisible({
    timeout: 10_000,
  });
  await page.getByRole("button", { name: /Import 1 Activit/i }).click();
  await expect(page.getByRole("heading", { name: "Import Complete" })).toBeVisible({
    timeout: 60_000,
  });
  await expect(page.locator('[aria-current="step"]')).toHaveCount(0);

  const search = await page.request.post(BASE_URL + "/api/v1/activities/search", {
    data: {
      page: 0,
      pageSize: 100,
      accountIdFilter: [accountId],
    },
  });
  expect(search.ok(), await search.text()).toBeTruthy();
  const saved = await search.json();
  const imported = saved.data.filter(
    (activity: { accountId: string }) => activity.accountId === accountId,
  );
  expect(imported).toHaveLength(1);
  expect(imported[0].comment).toBe("synthetic addon handoff");
});
