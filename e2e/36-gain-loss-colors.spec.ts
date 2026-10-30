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
    const control = page.getByRole("group", { name: "Gain and loss colors", exact: true });
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
      ["Red up, green down", "red-up"],
      ["Red up, blue down", "red-up-blue-down"],
      ["Green up, red down", "green-up"],
    ] as const) {
      // The ready control, not an optional startup request, gates this setting change.
      const option = control.getByRole("button", { name: label, exact: true });
      await expect(option).toBeEnabled();
      checkingSave = true;
      const saved = page.waitForResponse(
        (response) =>
          response.request().method() === "PUT" &&
          new URL(response.url()).pathname.endsWith("/settings"),
      );
      await option.click();
      const response = await saved;
      expect(response.status(), await response.text()).toBe(200);
      expect(response.request().postDataJSON()).toEqual({ gainColors: scheme });
      expect(await response.json()).toMatchObject({ gainColors: scheme });
      await expect(option).toHaveAttribute("aria-pressed", "true");
      await expect(page.locator("html")).toHaveAttribute("data-gain-colors", scheme);
      checkingSave = false;
      expect(refreshes).toEqual([]);
      await page.reload();
      await expect(option).toBeEnabled();
      await expect(option).toHaveAttribute("aria-pressed", "true");
      await expect(page.locator("html")).toHaveAttribute("data-gain-colors", scheme);
    }
    expect(refreshes).toEqual([]);
  });
}
