import { generateKeyPairSync } from "node:crypto";
import { expect, test } from "@playwright/test";
import { BASE_URL, loginIfNeeded } from "./helpers";

for (const direction of ["ltr", "rtl"]) {
  test(`Futu credentials and fixture history (${direction})`, async ({ page }) => {
    test.setTimeout(180_000);
    await loginIfNeeded(page);
    const api = `${BASE_URL}/api/v1`;
    const readSettings = async () => (await page.request.get(`${api}/providers/settings`)).json();
    const original = (await readSettings()).find((row: { id: string }) => row.id === "FUTU");
    expect(original).toBeTruthy();
    expect(original.enabled).toBe(false);
    await page.goto(`${BASE_URL}/settings/market-data?dir=${direction}`);
    await page.getByRole("button", { name: "Futu settings", exact: true }).click();
    await expect(page.getByLabel("Private key (PEM)")).toHaveCount(0);
    await page.getByRole("button", { name: "Show / edit credentials" }).click();
    const privateKey = generateKeyPairSync("ed25519")
      .privateKey.export({ type: "pkcs8", format: "pem" })
      .toString();
    await page.getByLabel("AppKeyId", { exact: true }).fill("synthetic-e2e-app");
    await page.getByLabel("Private key (PEM)", { exact: true }).fill(privateKey);
    await page.getByRole("button", { name: "Save", exact: true }).click();
    await expect(page.getByRole("status")).toHaveText("Credentials saved.");
    await page.getByRole("switch", { name: "Futu", exact: true }).click();
    await expect
      .poll(
        async () => (await readSettings()).find((p: { id: string }) => p.id === "FUTU")?.hasApiKey,
      )
      .toBe(true);
    await page.reload();
    await page.getByRole("button", { name: "Futu settings", exact: true }).click();
    await page.getByRole("button", { name: "Show / edit credentials" }).click();
    await expect(page.getByLabel("AppKeyId", { exact: true })).toHaveValue("synthetic-e2e-app");
    await expect(page.getByLabel("Private key (PEM)", { exact: true })).toHaveValue(
      privateKey.trim(),
    );
    await page.getByRole("button", { name: "Hide credentials" }).click();
    await expect(page.getByLabel("Private key (PEM)")).toHaveCount(0);

    const created = await page.request.post(`${api}/assets`, {
      data: {
        kind: "INVESTMENT",
        name: "Futu fixture Apple",
        quoteMode: "MARKET",
        quoteCcy: "USD",
        instrumentType: "EQUITY",
        instrumentSymbol: "AAPL",
        instrumentExchangeMic: "XNAS",
        providerConfig: { preferred_provider: "FUTU" },
      },
    });
    expect(created.ok()).toBeTruthy();
    const asset = await created.json();
    try {
      expect((await page.request.post(`${api}/market-data/sync/history`)).ok()).toBeTruthy();
      await expect
        .poll(
          async () => {
            const quotes = await (
              await page.request.get(`${api}/market-data/quotes/history`, {
                params: { symbol: asset.id },
              })
            ).json();
            return quotes.some((quote: { dataSource: string }) => quote.dataSource === "FUTU");
          },
          { timeout: 30_000 },
        )
        .toBe(true);
      const assetRow = (await (await page.request.get(`${api}/assets`)).json()).find(
        (row: { id: string }) => row.id === asset.id,
      );
      expect(assetRow.providerConfig.preferred_provider).toBe("FUTU");
    } finally {
      expect((await page.request.delete(`${api}/assets/${asset.id}`)).ok()).toBeTruthy();
      await page.getByRole("button", { name: "Show / edit credentials" }).click();
      await page.getByLabel("Private key (PEM)", { exact: true }).fill("invalid-pem");
      await page.getByRole("button", { name: "Save", exact: true }).click();
      await expect(page.getByRole("status")).toContainText("validation or client refresh failed");
      await page.getByRole("switch", { name: "Futu", exact: true }).click();
      await expect
        .poll(
          async () => (await readSettings()).find((p: { id: string }) => p.id === "FUTU")?.enabled,
        )
        .toBe(false);
      await page.getByRole("button", { name: "Delete credentials" }).click();
      await expect(page.getByRole("status")).toHaveText("Credentials deleted.");
      const restored = await page.request.put(`${api}/providers/settings`, {
        data: { providerId: "FUTU", priority: original.priority, enabled: false },
      });
      expect(restored.ok()).toBeTruthy();
    }
  });
}
