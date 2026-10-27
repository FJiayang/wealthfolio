import { generateKeyPairSync } from "node:crypto";
import { expect, test } from "@playwright/test";
import { BASE_URL, loginIfNeeded } from "./helpers";

for (const direction of ["ltr", "rtl"]) {
  test(`Futu credentials and fixture history (${direction})`, async ({ page }) => {
    test.setTimeout(180_000);
    await loginIfNeeded(page);
    const api = `${BASE_URL}/api/v1`;
    const readSettings = async () => (await page.request.get(`${api}/providers/settings`)).json();
    const settings = await readSettings();
    const original = settings.find((row: { id: string }) => row.id === "FUTU");
    const eastmoney = settings.find((row: { id: string }) => row.id === "EASTMONEY");
    expect(original).toBeTruthy();
    expect(original.enabled).toBe(false);
    const originalSecret = await (
      await page.request.get(`${api}/secrets`, {
        params: { secretKey: "FUTU" },
      })
    ).json();
    const existingAssets = await (await page.request.get(`${api}/assets`)).json();
    let asset: { id: string } | undefined;
    let failed = false;
    try {
      // Search fallback assertions must not depend on providers enabled by another test.
      for (const provider of settings.filter((row: { id: string }) =>
        ["TENCENT", "SINA"].includes(row.id),
      )) {
        expect(
          (
            await page.request.put(`${api}/providers/settings`, {
              data: { providerId: provider.id, priority: provider.priority, enabled: false },
            })
          ).ok(),
        ).toBeTruthy();
      }
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
          async () =>
            (await readSettings()).find((p: { id: string }) => p.id === "FUTU")?.hasApiKey,
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

      const search = await page.request.get(`${api}/market-data/search`, {
        params: { query: "XNAS:AAPL" },
      });
      expect(search.ok()).toBeTruthy();
      expect(
        (await search.json()).some(
          (row: { canonicalSymbol: string; exchangeMic: string }) =>
            row.canonicalSymbol === "AAPL" && row.exchangeMic === "XNAS",
        ),
      ).toBe(true);

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
      asset = await created.json();
      const assetId = asset!.id;
      const previous = existingAssets.find((row: { id: string }) => row.id === assetId);
      if (previous) {
        // Creating a known instrument preserves its existing configuration. Select
        // this test's provider explicitly, then restore the original in finally.
        const configured = await page.request.put(`${api}/assets/profile/${assetId}`, {
          data: {
            ...previous,
            notes: previous.notes ?? "",
            providerConfig: { ...previous.providerConfig, preferred_provider: "FUTU" },
          },
        });
        expect(configured.ok()).toBeTruthy();
        expect((await configured.json()).id).toBe(assetId);
      }
      expect(
        (
          await page.request.put(`${api}/providers/settings`, {
            data: {
              providerId: "EASTMONEY",
              priority: eastmoney.priority,
              enabled: true,
            },
          })
        ).ok(),
      ).toBeTruthy();
      for (const [query, canonical, mic] of [
        ["FUND:161725", "FUND:161725", null],
        ["XSHE:161725", "161725", "XSHE"],
      ] as const) {
        const response = await page.request.get(`${api}/market-data/search`, { params: { query } });
        expect(response.ok()).toBeTruthy();
        const rows = await response.json();
        expect(rows).toHaveLength(1);
        expect(rows[0]).toMatchObject({
          canonicalSymbol: canonical,
          dataSource: "EASTMONEY",
        });
        expect(rows[0].exchangeMic ?? null).toBe(mic);
      }
      expect((await page.request.post(`${api}/market-data/sync/history`)).ok()).toBeTruthy();
      await expect
        .poll(
          async () => {
            const quotes = await (
              await page.request.get(`${api}/market-data/quotes/history`, {
                params: { symbol: assetId },
              })
            ).json();
            return quotes.some((quote: { dataSource: string }) => quote.dataSource === "FUTU");
          },
          { timeout: 30_000 },
        )
        .toBe(true);
      const assetRow = (await (await page.request.get(`${api}/assets`)).json()).find(
        (row: { id: string }) => row.id === assetId,
      );
      expect(assetRow.providerConfig.preferred_provider).toBe("FUTU");

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
    } catch (error) {
      failed = true;
      throw error;
    } finally {
      const cleanupErrors: unknown[] = [];
      const cleanup = async (restore: () => Promise<void>) => {
        try {
          await restore();
        } catch (error) {
          cleanupErrors.push(error);
        }
      };
      if (asset)
        await cleanup(async () => {
          const previous = existingAssets.find((row: { id: string }) => row.id === asset!.id);
          if (previous) {
            expect(
              (
                await page.request.put(`${api}/assets/profile/${asset!.id}`, {
                  data: {
                    ...previous,
                    notes: previous.notes ?? "",
                    providerConfig: previous.providerConfig ?? {},
                  },
                })
              ).ok(),
            ).toBeTruthy();
          } else {
            expect((await page.request.delete(`${api}/assets/${asset!.id}`)).ok()).toBeTruthy();
          }
        });
      await cleanup(async () => {
        const response =
          originalSecret == null
            ? await page.request.delete(`${api}/secrets`, { params: { secretKey: "FUTU" } })
            : await page.request.post(`${api}/secrets`, {
                data: { secretKey: "FUTU", secret: originalSecret },
              });
        expect(response.ok()).toBeTruthy();
      });
      for (const provider of settings)
        await cleanup(async () => {
          expect(
            (
              await page.request.put(`${api}/providers/settings`, {
                data: {
                  providerId: provider.id,
                  priority: provider.priority,
                  enabled: provider.enabled,
                },
              })
            ).ok(),
          ).toBeTruthy();
        });
      if (cleanupErrors.length) {
        if (!failed) throw new AggregateError(cleanupErrors, "Fixture cleanup failed");
        console.error("Fixture cleanup also failed", cleanupErrors);
      }
    }
  });
}
