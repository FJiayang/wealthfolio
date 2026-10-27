import { expect, test } from "@playwright/test";
import { completeOnboardingIfNeeded, gotoAppPath } from "./helpers";

for (const width of [390, 1280]) {
  test(`gain/loss colors save and reload at ${width}px without market refresh`, async ({
    page,
  }) => {
    test.setTimeout(120000);
    await page.setViewportSize({ width, height: 900 });
    await completeOnboardingIfNeeded(page);
    await gotoAppPath(page, "/settings/appearance?dir=rtl");
    const select = page.getByTestId("gain-loss-color-select");
    const refreshes: string[] = [];
    let checkingSave = false;
    page.on("request", (request) => {
      if (
        checkingSave &&
        request.method() === "POST" &&
        /\/(portfolio\/(update|recalculate)|market-data\/sync)(\/|$)/.test(
          new URL(request.url()).pathname,
        )
      ) {
        refreshes.push(request.url());
      }
    });
    for (const [label, scheme] of [
      ["Red up, green down", "red-up-green-down"],
      ["Green up, red down", "green-up-red-down"],
    ] as const) {
      // The ready control, not an optional startup request, gates this setting change.
      await expect(select).toBeEnabled();
      checkingSave = true;
      await select.click();
      const saved = page.waitForResponse(
        (response) =>
          response.request().method() === "PUT" &&
          new URL(response.url()).pathname.endsWith("/settings"),
      );
      await page.getByRole("option", { name: label, exact: true }).press("Enter");
      const response = await saved;
      expect(response.status(), await response.text()).toBe(200);
      expect(response.request().postDataJSON()).toEqual({ gainLossColorScheme: scheme });
      expect(await response.json()).toMatchObject({ gainLossColorScheme: scheme });
      await expect(select).toHaveText(label);
      await expect
        .poll(() =>
          page.locator("html").evaluate((root) => root.classList.contains("red-up-green-down")),
        )
        .toBe(scheme === "red-up-green-down");
      checkingSave = false;
      expect(refreshes).toEqual([]);
      await page.reload();
      await expect(select).toBeEnabled();
      await expect(select).toHaveText(label);
      await expect
        .poll(() =>
          page.locator("html").evaluate((root) => root.classList.contains("red-up-green-down")),
        )
        .toBe(scheme === "red-up-green-down");
    }
    expect(refreshes).toEqual([]);
  });
}
