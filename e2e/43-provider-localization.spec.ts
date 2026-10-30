import { expect } from "@playwright/test";
import { test, addProviderFromCatalog } from "./provider-test-fixture";
import { BASE_URL, completeOnboardingIfNeeded } from "./helpers";

test("market-data metadata translates while provider brands stay unchanged", async ({
  page,
  providerCleanup,
}) => {
  test.setTimeout(120000);
  await completeOnboardingIfNeeded(page);
  const api = `${BASE_URL}/api/v1`;
  const settingsResponse = await page.request.get(`${api}/settings`);
  expect(settingsResponse.ok()).toBe(true);
  const settings = await settingsResponse.json();
  const providersResponse = await page.request.get(`${api}/providers/settings`);
  expect(providersResponse.ok()).toBe(true);
  const yahoo = (await providersResponse.json()).find(
    (provider: { id: string }) => provider.id === "YAHOO",
  );
  expect(yahoo).toBeDefined();
  providerCleanup(async (request) => {
    const restored = await request.put(`${api}/settings`, {
      data: { language: settings.language },
    });
    expect(restored.ok()).toBe(true);
  });
  if (!yahoo.enabled) {
    providerCleanup(async (request) => {
      const restored = await request.put(`${api}/providers/settings`, {
        data: { providerId: yahoo.id, enabled: yahoo.enabled, priority: yahoo.priority },
      });
      expect(restored.ok()).toBe(true);
    });
    await page.goto(`${BASE_URL}/settings/market-data`);
    await addProviderFromCatalog(page, "Yahoo Finance");
    await expect(page.getByRole("switch", { name: "Yahoo Finance", exact: true })).toBeChecked();
  }

  const saved = await page.request.put(`${api}/settings`, { data: { language: "zh" } });
  expect(saved.ok()).toBe(true);
  await page.goto(`${BASE_URL}/settings/market-data?dir=rtl`);
  await expect(page.getByText("Yahoo Finance", { exact: true })).toBeVisible();
  // The description and features sit in the provider's settings.
  await page.getByRole("button", { name: "Yahoo Finance 设置", exact: true }).click();
  await expect(
    page.getByText("Yahoo Finance 提供多个市场的历史及实时股票数据。", { exact: true }),
  ).toBeVisible();
  await expect(page.getByText("股息", { exact: true }).first()).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
});
