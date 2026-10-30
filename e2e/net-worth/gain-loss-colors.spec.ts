import { expect, test } from "@playwright/test";

for (const width of [390, 1280]) {
  for (const theme of ["light", "dark"]) {
    test(`net-worth gain colors switch without changing category colors at ${width}px in ${theme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/e2e/net-worth/?language=en");
      await page.locator("html").evaluate((root, mode) => {
        root.classList.remove("dark", "light");
        root.classList.add(mode);
        root.setAttribute("dir", "rtl");
        root.setAttribute("data-gain-colors", "green-up");
      }, theme);
      const gain = page.locator(".text-gain").first();
      const asset = page.locator(".text-success").first();
      const liability = page.locator(".text-destructive").first();
      await expect(gain).toBeAttached();
      const originalGain = await gain.evaluate((element) => getComputedStyle(element).color);
      const originalAsset = await asset.evaluate((element) => getComputedStyle(element).color);
      const originalLiability = await liability.evaluate(
        (element) => getComputedStyle(element).color,
      );
      expect(originalGain).toBe(originalAsset);
      await page
        .locator("html")
        .evaluate((root) => root.setAttribute("data-gain-colors", "red-up"));
      await expect(gain).toHaveCSS("color", originalLiability);
      await expect(asset).toHaveCSS("color", originalAsset);
      await expect(liability).toHaveCSS("color", originalLiability);
      await page.screenshot({ path: test.info().outputPath("red-up.png") });
      await page
        .locator("html")
        .evaluate((root) => root.setAttribute("data-gain-colors", "green-up"));
      await expect(gain).toHaveCSS("color", originalGain);
    });
  }
}
