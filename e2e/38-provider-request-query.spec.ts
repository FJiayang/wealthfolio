import { expect, test } from "@playwright/test";
import { BASE_URL, completeOnboardingIfNeeded } from "./helpers";

for (const width of [1280, 390]) {
  for (const rtl of [false, true]) {
    test(`provider request drawer at ${width}px, ${rtl ? "RTL" : "LTR"}`, async ({
      page,
    }, testInfo) => {
      test.setTimeout(120_000);
      await page.setViewportSize({ width, height: 900 });
      await completeOnboardingIfNeeded(page);
      await page.goto(`${BASE_URL}/settings/market-data${rtl ? "?dir=rtl" : ""}`);
      await expect(page.locator("html")).toHaveAttribute("dir", rtl ? "rtl" : "ltr");
      await expect(page.getByRole("heading", { name: "Provider requests" })).toHaveCount(0);
      await page.getByRole("button", { name: "Query details", exact: true }).click();
      const drawer = page.getByRole("dialog", { name: "Query details" });
      await expect(drawer).toBeVisible();
      await expect(drawer.locator("#request-count-range")).toContainText("30");
      await drawer.getByRole("button", { name: "Close", exact: true }).click();
      await expect(drawer).not.toBeVisible();

      const today = page.getByRole("button", { name: /^Today so far:/ }).first();
      await expect(today).toBeEnabled();
      const providerName = await today
        .locator("..")
        .locator("span.font-medium")
        .first()
        .innerText();
      await today.click();
      await expect(drawer).toBeVisible();
      await expect(drawer.locator("#request-count-provider")).toContainText(providerName);
      await expect(drawer.locator("#request-count-range")).toHaveText("Today");
      await expect(drawer.locator("tbody tr")).toHaveCount(1);
      await expect(drawer).toBeInViewport({ ratio: 0.99 });
      expect(
        await drawer.evaluate((element) => element.scrollWidth <= element.clientWidth + 1),
      ).toBe(true);
      await page.screenshot({
        path: testInfo.outputPath("request-drawer.png"),
        animations: "disabled",
      });
      await page.keyboard.press("Escape");
      await expect(drawer).not.toBeVisible();
    });
  }
}
