import { createServer } from "node:http";
import { expect, test } from "@playwright/test";
import { BASE_URL, completeOnboardingIfNeeded } from "./helpers";

for (const width of [1280, 390]) {
  for (const rtl of [false, true]) {
    test(`provider request details at ${width}px, ${rtl ? "RTL" : "LTR"}`, async ({
      page,
    }, testInfo) => {
      test.setTimeout(120_000);
      await page.setViewportSize({ width, height: 900 });
      await completeOnboardingIfNeeded(page);
      const source = createServer((_request, response) => {
        response.writeHead(429, { "Content-Type": "application/json" });
        response.end('{"error":"private-response"}');
      });
      await new Promise<void>((resolve) => source.listen(0, "127.0.0.1", resolve));
      try {
        const address = source.address();
        if (!address || typeof address === "string") throw new Error("Missing test port");
        const response = await page.request.post(
          `${BASE_URL}/api/v1/custom-providers/test-source`,
          {
            data: {
              providerId: "diagnostic-ui",
              format: "json",
              pricePath: "price",
              url: `http://127.0.0.1:${address.port}/?apikey=private-key`,
              headers: '{"Authorization":"Bearer private-key"}',
              symbol: "UI-TEST",
              currency: "usd",
            },
          },
        );
        expect(response.ok()).toBe(true);
      } finally {
        await new Promise<void>((resolve, reject) =>
          source.close((error) => (error ? reject(error) : resolve())),
        );
      }

      await page.goto(`${BASE_URL}/settings/market-data${rtl ? "?dir=rtl" : ""}`);
      await page.getByRole("button", { name: "Query details", exact: true }).click();
      await expect(page).toHaveURL(/\/settings\/market-data\/requests/);
      await expect(page.locator("html")).toHaveAttribute("dir", rtl ? "rtl" : "ltr");
      await page.getByLabel("Security code (exact)").fill("UI-TEST");
      const row = page.getByRole("row").filter({ hasText: "UI-TEST" }).first();
      await expect(row).toContainText("429");
      await expect(row).toContainText(/\d+[\u2066-\u2069]* ms/);
      await expect(row).toContainText("test_source");
      await row.scrollIntoViewIfNeeded();
      await expect(row.getByRole("button", { name: /View details/ })).toBeInViewport();
      await page.screenshot({
        path: testInfo.outputPath("request-list.png"),
        animations: "disabled",
      });
      await row.getByRole("button", { name: /View details/ }).click();
      const detail = page.getByRole("dialog", { name: "Request details" });
      await expect(detail).toBeInViewport({ ratio: 0.99 });
      await expect(detail).toContainText("UI-TEST");
      await expect(detail).toContainText("http_status");
      await expect(detail.locator("pre")).toContainText('"currency": "usd"');
      await expect(detail).not.toContainText("private-key");
      await expect(detail).not.toContainText("private-response");
      expect(
        await detail.evaluate((element) => element.scrollWidth <= element.clientWidth + 1),
      ).toBe(true);
      await page.screenshot({
        path: testInfo.outputPath("request-detail.png"),
        animations: "disabled",
      });
      await page.keyboard.press("Escape");
      await expect(detail).not.toBeVisible();
      await page.getByRole("button", { name: "Back", exact: true }).last().click();

      const today = page.getByRole("button", { name: /^Today so far:/ }).first();
      await expect(today).toBeEnabled();
      await today.click();
      const url = new URL(page.url());
      expect(url.searchParams.get("providerId")).toBeTruthy();
      expect(url.searchParams.get("from")).toBe(url.searchParams.get("to"));
      await expect(page.locator("html")).toHaveAttribute("dir", rtl ? "rtl" : "ltr");
    });
  }
}
