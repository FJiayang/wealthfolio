import { expect } from "@playwright/test";
import { test } from "./provider-test-fixture";
import { BASE_URL, loginIfNeeded } from "./helpers";

for (const direction of ["ltr", "rtl"]) {
  test(`IFIND search, creation and persisted quotes (${direction})`, async ({
    page,
    providerCleanup,
  }) => {
    test.setTimeout(180000);
    await loginIfNeeded(page);
    const api = `${BASE_URL}/api/v1`;
    const settings = await (await page.request.get(`${api}/providers/settings`)).json();
    const existingAssets = await (await page.request.get(`${api}/assets`)).json();
    const created: string[] = [];
    providerCleanup(async (request) => {
      const cleanupErrors: unknown[] = [];
      const cleanup = async (restore: () => Promise<void>) => {
        try {
          await restore();
        } catch (error) {
          cleanupErrors.push(error);
        }
      };
      for (const id of created)
        await cleanup(async () => {
          expect((await request.delete(`${api}/assets/${id}`)).ok()).toBeTruthy();
        });
      for (const provider of settings)
        await cleanup(async () => {
          expect(
            (
              await request.put(`${api}/providers/settings`, {
                data: {
                  providerId: provider.id,
                  priority: provider.priority,
                  enabled: provider.enabled,
                },
              })
            ).ok(),
          ).toBeTruthy();
        });
      if (cleanupErrors.length) throw new AggregateError(cleanupErrors, "Fixture cleanup failed");
    });
    for (const provider of settings) {
      const updated = await page.request.put(`${api}/providers/settings`, {
        data: {
          providerId: provider.id,
          priority: provider.priority,
          enabled: provider.id === "IFIND",
        },
      });
      expect(updated.ok()).toBeTruthy();
    }
    await page.goto(`${BASE_URL}/settings/market-data?dir=${direction}`);
    await expect(
      page.getByRole("switch", { name: "Tonghuashun iFinD", exact: true }),
    ).toBeChecked();
    for (const [query, symbol, mic, currency] of [
      ["XSHG:600519", "600519", "XSHG", "CNY"],
      ["FUND:005827", "FUND:005827", null, "CNY"],
      ["000300", "000300", "XSHG", "CNY"],
      ["XHKG:00700", "00700", "XHKG", "HKD"],
      ["XNAS:INTU", "INTU", "XNAS", "USD"],
    ] as const) {
      await page.goto(`${BASE_URL}/settings/securities?dir=${direction}`);
      await page.getByRole("button", { name: "Add security", exact: true }).click();
      const dialog = page.getByRole("dialog", { name: "Add security", exact: true });
      await dialog.getByRole("combobox", { name: "Search", exact: true }).click();
      await page.getByPlaceholder("Search for symbol").fill(query);
      await page
        .getByRole("listbox", { name: /Suggestions/i })
        .getByRole("option")
        .filter({ hasText: query, hasNotText: /Create custom|manual/i })
        .first()
        .click();
      await expect(dialog.getByPlaceholder("e.g., AAPL")).toHaveValue(symbol);
      const creationResponse = page.waitForResponse(
        (response) =>
          response.url().endsWith("/api/v1/assets") && response.request().method() === "POST",
      );
      await dialog.getByRole("button", { name: "Create security", exact: true }).click();
      const response = await creationResponse;
      const createdAsset = await response.json();
      // Register ownership before UI/assertion failures can interrupt cleanup.
      if (
        typeof createdAsset.id === "string" &&
        !existingAssets.some((row: { id: string }) => row.id === createdAsset.id)
      ) {
        created.push(createdAsset.id);
      }
      expect(response.ok()).toBeTruthy();
      await expect(dialog).not.toBeVisible();
      const assets = await (await page.request.get(`${api}/assets`)).json();
      const asset = assets.find(
        (row: { instrumentSymbol: string; instrumentExchangeMic: string | null }) =>
          row.instrumentSymbol === symbol && (row.instrumentExchangeMic ?? null) === mic,
      );
      expect(asset).toBeDefined();
      if (
        !existingAssets.some((row: { id: string }) => row.id === asset.id) &&
        !created.includes(asset.id)
      )
        created.push(asset.id);
      expect(asset).toMatchObject({ quoteCcy: currency, quoteMode: "MARKET" });
      expect(asset.providerConfig.preferred_provider).toBe("IFIND");
      if (symbol === "000300")
        expect(asset.providerConfig.overrides.IFIND.symbol).toBe("index:000300.SH");
      expect((await page.request.post(`${api}/market-data/sync/history`)).ok()).toBeTruthy();
      await expect
        .poll(
          async () => {
            const quotes = await (
              await page.request.get(`${api}/market-data/quotes/history`, {
                params: { symbol: asset.id },
              })
            ).json();
            return quotes.some(
              (q: { dataSource: string; currency: string }) =>
                q.dataSource === "IFIND" && q.currency === currency,
            );
          },
          { timeout: 30000 },
        )
        .toBe(true);
    }
  });
}
