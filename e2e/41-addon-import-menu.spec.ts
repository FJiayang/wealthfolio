import { devices, expect, test } from "@playwright/test";
import { randomUUID } from "node:crypto";
import { importAddonFixture } from "./fixtures/import-addons/fixture";
import { BASE_URL, completeOnboardingIfNeeded } from "./helpers";

function defineImportMenuTests(mobile: boolean) {
  for (const direction of ["ltr", "rtl"]) {
    test(`fixture import addons open from activity menus (${mobile ? "mobile" : "desktop"}, ${direction})`, async ({
      page,
    }, testInfo) => {
      test.setTimeout(180_000);
      await completeOnboardingIfNeeded(page);
      await page.setViewportSize({ width: mobile ? 390 : 1440, height: 900 });
      const api = `${BASE_URL}/api/v1`;
      const state = await (
        await page.request.post(`${api}/profiles/get_profile_state`, { data: {} })
      ).json();
      const unlocked = await page.request.post(`${api}/profiles/unlock_profile`, {
        data: { profileId: state.profiles[0].id },
      });
      expect(unlocked.ok()).toBe(true);
      const session = await unlocked.json();
      const headers = { "x-wf-profile-scope": session.scopeId };
      const settingsResponse = await page.request.get(`${api}/settings`, { headers });
      const spendingResponse = await page.request.get(`${api}/spending/settings`, { headers });
      expect(settingsResponse.ok()).toBe(true);
      expect(spendingResponse.ok()).toBe(true);
      const original = await settingsResponse.json();
      const originalSpending = await spendingResponse.json();
      const ownedAddons = new Set<string>();
      let accountId: string | undefined;
      let failed = false;
      try {
        expect(
          (
            await page.request.put(`${api}/settings`, {
              headers,
              data: { language: "en", syncEnabled: false, autoUpdateCheckEnabled: false },
            })
          ).ok(),
        ).toBe(true);
        expect(
          (
            await page.request.put(`${api}/spending/settings`, {
              headers,
              data: { enabled: true, accountIds: [] },
            })
          ).ok(),
        ).toBe(true);
        const accountResponse = await page.request.post(`${api}/accounts`, {
          headers,
          data: {
            name: `Import fixture ${randomUUID()}`,
            accountType: "SECURITIES",
            currency: "USD",
            isDefault: false,
            isActive: true,
            trackingMode: "TRANSACTIONS",
          },
        });
        const account = await accountResponse.json();
        if (typeof account.id === "string") accountId = account.id;
        expect(accountResponse.ok()).toBe(true);
        expect(accountId).toEqual(expect.any(String));
        const addons = (["investments", "spending"] as const).map((tab) =>
          importAddonFixture(`host-import-${tab}-${randomUUID()}`, tab, accountId!),
        );
        for (const addon of addons) {
          // Even an interrupted installation can leave this uniquely owned ID behind.
          ownedAddons.add(addon.id);
          const installed = await page.request.post(`${api}/addons/install-zip`, {
            headers,
            data: {
              zipDataB64: addon.zip.toString("base64"),
              enableAfterInstall: true,
            },
          });
          expect(installed.ok(), await installed.text()).toBe(true);
        }
        for (const addon of addons) {
          await page.goto(`${BASE_URL}/activities?tab=${addon.tab}&dir=${direction}`);
          const trigger = mobile
            ? page.getByRole("button", { name: "Import", exact: true }).filter({ visible: true })
            : page.getByTestId("add-activities-button").filter({ visible: true });
          await expect(trigger).toBeVisible();
          await expect(page.locator(`nav a[href="/addons/${addon.id}"]`)).toHaveCount(0);
          await expect
            .poll(() =>
              page.evaluate(async (id) => {
                const modulePath = "/src/addons/iframe/addon-iframe-manager.ts";
                const { addonIframeManager } = await import(modulePath);
                return addonIframeManager.hasRuntime(id);
              }, addon.id),
            )
            .toBe(false);
          await trigger.click();
          const entry = page.getByTestId(`addon-import-${addon.id}:${addon.route}`);
          await expect(entry).toBeVisible();
          const other = addons.find((candidate) => candidate.id !== addon.id)!;
          await expect(page.getByTestId(`addon-import-${other.id}:${other.route}`)).toHaveCount(0);
          await entry.focus();
          await page.keyboard.press("Enter");
          await expect(page).toHaveURL(new RegExp(`/addons/${addon.id}$`));
          await expect(
            page
              .frameLocator("iframe")
              .getByRole("button", { name: "Open host import wizard", exact: true }),
          ).toBeVisible({ timeout: 30_000 });

          const frame = page.frameLocator("iframe");
          await expect(page.locator("html")).toHaveAttribute("dir", direction);
          await expect(frame.locator("html")).toHaveAttribute("dir", direction);
          await frame.getByRole("button", { name: "Open host import wizard", exact: true }).click();
          await expect(page).toHaveURL(/\/import\?addonImport=1$/);
          await expect(page.getByRole("button", { name: /Review Activities/i })).toBeEnabled({
            timeout: 30_000,
          });
          await expect(page.locator("html")).toHaveAttribute("dir", direction);
          await page.screenshot({
            path: testInfo.outputPath(`${addon.tab}-host-handoff-${direction}.png`),
          });
          const cancelImport = mobile
            ? page.locator("header").getByRole("button", { name: "Back", exact: true })
            : page.getByRole("button", { name: "Return to original transactions", exact: true });
          await expect(cancelImport).toBeVisible();
          await cancelImport.click();
          await expect(page.getByRole("alertdialog")).toContainText("discards edits");
          await page.getByRole("button", { name: "Continue Importing", exact: true }).click();
          await expect(page.getByRole("button", { name: /Review Activities/i })).toBeEnabled();
          await cancelImport.click();
          await page
            .getByRole("alertdialog")
            .getByRole("button", { name: "Cancel Import", exact: true })
            .click();
          await expect(page).toHaveURL(new RegExp(`/addons/${addon.id}$`));
          await expect(
            frame.getByRole("button", { name: "Open host import wizard", exact: true }),
          ).toBeVisible();
          // Handoff and cancel must not persist either synthetic draft.
          const activities = await page.request.post(`${api}/activities/search`, {
            headers,
            data: { page: 0, pageSize: 10, accountIdFilter: [accountId] },
          });
          expect(activities.ok()).toBe(true);
          expect((await activities.json()).data).toEqual([]);
        }
        // Reload rebuilds menu contributions from enabled manifests, without sidebar links.
        const addon = addons[0];
        const disabled = await page.request.post(`${BASE_URL}/api/v1/addons/toggle`, {
          headers,
          data: { addonId: addon.id, enabled: false },
        });
        expect(disabled.ok()).toBe(true);
        await page.goto(`${BASE_URL}/activities?tab=investments&dir=${direction}`);
        if (mobile) {
          await expect(
            page.getByRole("link", { name: "Import", exact: true }).filter({ visible: true }),
          ).toHaveAttribute("href", "/import");
        } else {
          await page.getByTestId("add-activities-button").filter({ visible: true }).click();
          await expect(page.getByTestId(`addon-import-${addon.id}:${addon.route}`)).toHaveCount(0);
        }
        const enabled = await page.request.post(`${BASE_URL}/api/v1/addons/toggle`, {
          headers,
          data: { addonId: addon.id, enabled: true },
        });
        expect(enabled.ok()).toBe(true);
        await page.reload();
        await (
          mobile
            ? page.getByRole("button", { name: "Import", exact: true }).filter({ visible: true })
            : page.getByTestId("add-activities-button").filter({ visible: true })
        ).click();
        await expect(page.getByTestId(`addon-import-${addon.id}:${addon.route}`)).toBeVisible();
        for (const installed of addons) {
          const removed = await page.request.delete(`${BASE_URL}/api/v1/addons/${installed.id}`, {
            headers,
          });
          expect(removed.ok()).toBe(true);
          ownedAddons.delete(installed.id);
        }
        await page.goto(`${BASE_URL}/activities?tab=spending&dir=${direction}`);
        if (mobile) {
          await expect(
            page.getByRole("link", { name: "Import", exact: true }).filter({ visible: true }),
          ).toHaveAttribute("href", "/import");
        } else {
          await page.getByTestId("add-activities-button").filter({ visible: true }).click();
          await expect(page.locator('[data-testid^="addon-import-"]')).toHaveCount(0);
        }
      } catch (error) {
        failed = true;
        throw error;
      } finally {
        const errors: unknown[] = [];
        const cleanup = async (step: () => Promise<void>) => {
          try {
            await step();
          } catch (error) {
            errors.push(error);
          }
        };
        for (const id of ownedAddons) {
          await cleanup(async () => {
            const response = await page.request.delete(`${api}/addons/${id}`, { headers });
            expect(response.ok() || response.status() === 404).toBe(true);
          });
        }
        if (accountId)
          await cleanup(async () => {
            expect(
              (await page.request.delete(`${api}/accounts/${accountId}`, { headers })).ok(),
            ).toBe(true);
          });
        await cleanup(async () => {
          expect(
            (
              await page.request.put(`${api}/spending/settings`, {
                headers,
                data: {
                  enabled: originalSpending.enabled,
                  accountIds: originalSpending.accountIds,
                },
              })
            ).ok(),
          ).toBe(true);
        });
        await cleanup(async () => {
          expect(
            (
              await page.request.put(`${api}/settings`, {
                headers,
                data: {
                  language: original.language,
                  syncEnabled: original.syncEnabled,
                  autoUpdateCheckEnabled: original.autoUpdateCheckEnabled,
                },
              })
            ).ok(),
          ).toBe(true);
        });
        if (errors.length) {
          testInfo.annotations.push({
            type: "fixture cleanup failure",
            description: errors.map(String).join("\n"),
          });
          if (!failed) throw new AggregateError(errors, "Fixture cleanup failed");
        }
      }
    });
  }
}

defineImportMenuTests(false);
test.describe("mobile device", () => {
  // The application detects mobile platform from the UA, not viewport width.
  // Keep a real phone context so the header's Back action is available.
  test.use({ userAgent: devices["Pixel 5"].userAgent, isMobile: true, hasTouch: true });
  defineImportMenuTests(true);
});
