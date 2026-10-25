import { expect, test } from "@playwright/test";
import { BASE_URL, loginIfNeeded } from "./helpers";

for (const direction of ["ltr", "rtl"]) {
  test(`IFIND search, creation and persisted quotes (${direction})`, async ({ page }) => {
    test.setTimeout(180000);
    await loginIfNeeded(page);
    const api = `${BASE_URL}/api/v1`;
    const settings = await (await page.request.get(`${api}/providers/settings`)).json();
    const created: string[] = [];
    try {
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
        // Keep provider creation independent of AAPL already owned by earlier suites.
        ["XNAS:INTU", "INTU", "XNAS", "USD"],
      ] as const) {
        await page.goto(`${BASE_URL}/settings/securities?dir=${direction}`);
        await page.getByRole("button", { name: "Add Security", exact: true }).click();
        const dialog = page.getByRole("dialog", { name: "Add Security", exact: true });
        await dialog.getByRole("combobox", { name: "Search", exact: true }).click();
        await page.getByPlaceholder("Search for symbol").fill(query);
        await page
          .getByRole("listbox", { name: /Suggestions/i })
          .getByRole("option")
          .filter({ hasText: query, hasNotText: /Create custom|manual/i })
          .first()
          .click();
        await expect(dialog.getByPlaceholder("e.g., AAPL")).toHaveValue(symbol);
        await dialog.getByRole("button", { name: "Create Security", exact: true }).click();
        await expect(dialog).not.toBeVisible();
        const assets = await (await page.request.get(`${api}/assets`)).json();
        const asset = assets.find(
          (row: { instrumentSymbol: string; instrumentExchangeMic: string | null }) =>
            row.instrumentSymbol === symbol && (row.instrumentExchangeMic ?? null) === mic,
        );
        expect(asset).toBeDefined();
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
    } finally {
      for (const id of created) await page.request.delete(`${api}/assets/${id}`);
      for (const provider of settings)
        await page.request.put(`${api}/providers/settings`, {
          data: { providerId: provider.id, priority: provider.priority, enabled: provider.enabled },
        });
    }
  });
}
