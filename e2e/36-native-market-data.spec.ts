import { expect } from "@playwright/test";
import { test, addProviderFromCatalog } from "./provider-test-fixture";
import { BASE_URL, loginIfNeeded } from "./helpers";

for (const direction of ["ltr", "rtl"]) {
  test(`Native providers automatic creation and local search (${direction})`, async ({
    page,
    providerCleanup,
  }) => {
    test.setTimeout(180000);
    await loginIfNeeded(page);
    const api = `${BASE_URL}/api/v1`;
    await page.goto(`${BASE_URL}/settings/market-data?dir=${direction}`);
    const settings = await (await page.request.get(`${api}/providers/settings`)).json();
    expect(settings.some((p: { id: string }) => p.id === "HTTP_API")).toBe(false);
    const createdIds: string[] = [];
    providerCleanup(async (request) => {
      const cleanupErrors: unknown[] = [];
      const cleanup = async (restore: () => Promise<void>) => {
        try {
          await restore();
        } catch (error) {
          cleanupErrors.push(error);
        }
      };
      for (const id of createdIds)
        await cleanup(async () => {
          expect((await request.delete(`${api}/assets/${id}`)).ok()).toBeTruthy();
        });
      for (const provider of settings.filter((row: { id: string }) =>
        ["EASTMONEY", "TENCENT", "SINA"].includes(row.id),
      ))
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
    for (const [id, name] of [
      ["EASTMONEY", "Eastmoney"],
      ["TENCENT", "Tencent Finance"],
      ["SINA", "Sina Finance"],
    ]) {
      if (
        !settings.find((provider: { id: string; enabled: boolean }) => provider.id === id)?.enabled
      ) {
        await addProviderFromCatalog(page, name);
      }
      await expect(page.getByRole("switch", { name, exact: true })).toBeChecked();
      await expect
        .poll(async () => {
          const rows = await (await page.request.get(`${api}/providers/settings`)).json();
          return rows.find((p: { id: string }) => p.id === id)?.enabled;
        })
        .toBe(true);
    }

    for (const [symbol, canonical, mic] of [
      ["XSHG:600519", "600519", "XSHG"],
      ["XSHG:510300", "510300", "XSHG"],
      ["XSHE:161725", "161725", "XSHE"],
      ["FUND:005827", "FUND:005827", null],
      ["FUND:161725", "FUND:161725", null],
    ] as const) {
      const existing = await (await page.request.get(`${api}/assets`)).json();
      if (
        !existing.some(
          (asset: { instrumentSymbol: string; instrumentExchangeMic: string | null }) =>
            asset.instrumentSymbol === canonical && (asset.instrumentExchangeMic ?? null) === mic,
        )
      ) {
        await page.goto(`${BASE_URL}/settings/securities?dir=${direction}`);
        await page.getByRole("button", { name: "Add security", exact: true }).click();
        const dialog = page.getByRole("dialog", { name: "Add security", exact: true });
        await dialog.getByRole("combobox", { name: "Search", exact: true }).click();
        if (symbol.startsWith("FUND:")) {
          await page.getByPlaceholder("Search for symbol").fill("XSHG:600519");
          await page
            .getByRole("listbox", { name: /Suggestions/i })
            .getByRole("option")
            .filter({ hasText: "贵州茅台", hasNotText: /Create custom|manual/i })
            .first()
            .click();
          await dialog.getByRole("combobox", { name: "Search", exact: true }).click();
        }
        await page.getByPlaceholder("Search for symbol").fill(symbol);
        await page
          .getByRole("listbox", { name: /Suggestions/i })
          .getByRole("option")
          .filter({
            has: page.locator("span.font-mono").filter({ hasText: new RegExp(`^${symbol}$`) }),
            hasNotText: /Create custom|manual/i,
          })
          .first()
          .click();
        await expect(dialog.getByPlaceholder("e.g., AAPL")).toHaveValue(canonical);
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
          !existing.some((row: { id: string }) => row.id === createdAsset.id)
        ) {
          createdIds.push(createdAsset.id);
        }
        expect(response.ok()).toBeTruthy();
        await expect(dialog).not.toBeVisible();
      }
      const search = await page.request.get(`${api}/market-data/search`, {
        params: { query: symbol },
      });
      expect(search.ok()).toBeTruthy();
      expect(
        (await search.json()).some((result: { isExisting: boolean }) => result.isExisting),
      ).toBe(true);
      const assets = await (await page.request.get(`${api}/assets`)).json();
      const matches = assets.filter(
        (asset: { instrumentSymbol: string; instrumentExchangeMic: string | null }) =>
          asset.instrumentSymbol === canonical && (asset.instrumentExchangeMic ?? null) === mic,
      );
      expect(matches).toHaveLength(1);
      if (
        !existing.some((asset: { id: string }) => asset.id === matches[0].id) &&
        !createdIds.includes(matches[0].id)
      )
        createdIds.push(matches[0].id);
      expect(matches[0]).toMatchObject({
        quoteCcy: "CNY",
        quoteMode: "MARKET",
      });
      expect(matches[0].providerConfig?.preferred_provider).toBeUndefined();
      const sync = await page.request.post(`${api}/market-data/sync/history`);
      expect(sync.ok()).toBeTruthy();
      await expect
        .poll(
          async () => {
            const quotes = await (
              await page.request.get(`${api}/market-data/quotes/history`, {
                params: { symbol: matches[0].id },
              })
            ).json();
            return quotes.some((quote: { dataSource: string }) =>
              ["EASTMONEY", "TENCENT", "SINA"].includes(quote.dataSource),
            );
          },
          { timeout: 30_000 },
        )
        .toBe(true);
    }
    const response = await page.request.put(`${api}/providers/settings`, {
      data: { providerId: "EASTMONEY", priority: 10, enabled: false },
    });
    expect(response.ok()).toBeTruthy();
    for (const query of ["蓝筹", "0058"]) {
      const search = await page.request.get(`${api}/market-data/search`, { params: { query } });
      expect(search.ok()).toBeTruthy();
      expect(
        (await search.json()).some(
          (result: { isExisting: boolean; canonicalSymbol: string }) =>
            result.isExisting && result.canonicalSymbol === "FUND:005827",
        ),
      ).toBe(true);
    }
  });
}
