import { expect, test } from "@playwright/test";
import { completeOnboardingIfNeeded, gotoAppPath } from "./helpers";

test("daily market sync time and timezone persist in LTR and RTL", async ({ page }) => {
  test.setTimeout(120000);
  await completeOnboardingIfNeeded(page);
  for (const [direction, hour, minute] of [
    ["ltr", "21", "35"],
    ["rtl", "23", "45"],
  ] as const) {
    await gotoAppPath(page, `/settings/market-data?dir=${direction}`);
    await expect(page.locator("html")).toHaveAttribute("dir", direction);
    for (const [label, value] of [
      ["Hour", hour],
      ["Minute", minute],
    ]) {
      const select = page.getByRole("combobox", { name: label, exact: true });
      await select.click();
      const saved = page.waitForResponse(
        (response) =>
          response.request().method() === "PUT" &&
          new URL(response.url()).pathname.endsWith("/settings"),
      );
      await page.getByRole("option", { name: value, exact: true }).click();
      expect((await saved).status()).toBe(200);
      await expect(select).toHaveText(value);
    }
    await page.reload();
    await expect(page.getByRole("combobox", { name: "Hour", exact: true })).toHaveText(hour);
    await expect(page.getByRole("combobox", { name: "Minute", exact: true })).toHaveText(minute);
    const timezone = page.getByRole("combobox", { name: "Timezone", exact: true }).first();
    await timezone.click();
    const saved = page.waitForResponse(
      (response) =>
        response.request().method() === "PUT" &&
        new URL(response.url()).pathname.endsWith("/settings"),
    );
    const target = direction === "ltr" ? "Asia/Tokyo" : "Asia/Shanghai";
    await page.getByRole("option", { name: target, exact: true }).click();
    expect(await (await saved).json()).toMatchObject({
      marketDataSyncTime: `${hour}:${minute}`,
      marketDataSyncTimezone: target,
    });
    await page.reload();
    await expect(timezone).toHaveText(target);
    const bounds = await timezone.boundingBox();
    expect(bounds!.x).toBeGreaterThanOrEqual(0);
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(page.viewportSize()!.width);
  }
});
