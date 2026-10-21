import { expect, test } from "@playwright/test";
import { BASE_URL, loginIfNeeded } from "./helpers";

for (const direction of ["ltr", "rtl"]) {
  test(`HTTP API exact creation and local search (${direction})`, async ({ page }) => {
    test.setTimeout(180000);
    await loginIfNeeded(page);
    const api = `${BASE_URL}/api/v1`;
    await page.goto(`${BASE_URL}/settings/market-data?dir=${direction}`);
    await page.getByRole("button", { name: "Standard HTTP API settings" }).click();
    const url = page.getByLabel("Service base URL");
    await url.fill("http://finance-data-api:8000");
    await url.locator("..").getByRole("button", { name: "Save", exact: true }).click();
    await expect
      .poll(async () => {
        const response = await page.request.get(`${api}/providers/settings`);
        const providers = await response.json();
        return providers.find((provider: { id: string }) => provider.id === "HTTP_API")?.baseUrl;
      })
      .toBe("http://finance-data-api:8000");
    const enabled = page.getByRole("switch", { name: "Standard HTTP API", exact: true });
    if ((await enabled.getAttribute("aria-checked")) === "false") await enabled.click();
    await expect
      .poll(async () => {
        const response = await page.request.get(`${api}/providers/settings`);
        const providers = await response.json();
        return providers.find((provider: { id: string }) => provider.id === "HTTP_API")?.enabled;
      })
      .toBe(true);

    const createdIds: string[] = [];
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
        await page.getByRole("button", { name: "Add Security", exact: true }).click();
        const dialog = page.getByRole("dialog", { name: "Add Security", exact: true });
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
        await dialog.getByRole("button", { name: "Create Security", exact: true }).click();
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
      if (!existing.some((asset: { id: string }) => asset.id === matches[0].id))
        createdIds.push(matches[0].id);
      expect(matches[0]).toMatchObject({
        quoteCcy: "CNY",
        quoteMode: "MARKET",
        providerConfig: { preferred_provider: "HTTP_API" },
      });
    }
    const response = await page.request.put(`${api}/providers/settings`, {
      data: { providerId: "HTTP_API", priority: 10, enabled: false },
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
    for (const id of createdIds) {
      const deleted = await page.request.delete(`${api}/assets/${id}`);
      expect(deleted.ok()).toBeTruthy();
    }
  });
}
