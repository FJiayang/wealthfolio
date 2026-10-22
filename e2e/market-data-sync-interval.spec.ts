import { expect, test } from "@playwright/test";
import { completeOnboardingIfNeeded, gotoAppPath } from "./helpers";

test("scheduled market sync interval persists in LTR and RTL", async ({ page }) => {
  test.setTimeout(120000);
  await completeOnboardingIfNeeded(page);

  for (const [direction, hours] of [
    ["ltr", 12],
    ["rtl", 24],
  ] as const) {
    await gotoAppPath(page, `/settings/market-data?dir=${direction}`);
    await expect(page.locator("html")).toHaveAttribute("dir", direction);
    const select = page.getByRole("combobox", { name: "Automatic sync interval" });
    await select.click();
    const saved = page.waitForResponse(
      (response) =>
        response.request().method() === "PUT" &&
        new URL(response.url()).pathname.endsWith("/settings"),
    );
    await page.getByRole("option", { name: `Every ${hours} hours`, exact: true }).click();
    const response = await saved;
    expect(response.request().postDataJSON()).toEqual({ marketDataSyncIntervalHours: hours });
    expect(response.status(), await response.text()).toBe(200);
    expect(await response.json()).toMatchObject({ marketDataSyncIntervalHours: hours });
    await expect(select).toHaveText(`Every ${hours} hours`);
    await page.reload();
    await expect(select).toHaveText(`Every ${hours} hours`);
    const bounds = await select.boundingBox();
    expect(bounds).not.toBeNull();
    expect(bounds!.x).toBeGreaterThanOrEqual(0);
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(page.viewportSize()!.width);
  }
});
