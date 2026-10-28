import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { BASE_URL } from "./helpers";

// These local addons live in a separate repository and are packaged before this suite runs.
const addonDirectory = process.env.WF_E2E_IMPORT_ADDONS_DIR;
const addons = [
  {
    id: "ant-fund-pdf-import-addon",
    route: "ant-fund-pdf-import",
    tab: "investments",
    upload: "Choose PDF file",
  },
  {
    id: "unified-statement-import-addon",
    route: "statement-import",
    tab: "spending",
    upload: "Choose a statement file",
  },
];

for (const mobile of [false, true]) {
  for (const direction of ["ltr", "rtl"]) {
    test(`local import addons open from activity menus (${mobile ? "mobile" : "desktop"}, ${direction})`, async ({
      page,
    }) => {
      test.skip(
        !addonDirectory,
        "Set WF_E2E_IMPORT_ADDONS_DIR to the packaged local addon directory",
      );
      test.setTimeout(180_000);
      await page.setViewportSize({ width: mobile ? 390 : 1440, height: 900 });
      const state = await (
        await page.request.post(`${BASE_URL}/api/v1/profiles/get_profile_state`, { data: {} })
      ).json();
      const unlocked = await page.request.post(`${BASE_URL}/api/v1/profiles/unlock_profile`, {
        data: { profileId: state.profiles[0].id },
      });
      expect(unlocked.ok()).toBe(true);
      const session = await unlocked.json();
      const headers = { "x-wf-profile-scope": session.scopeId };
      const settings = await page.request.put(`${BASE_URL}/api/v1/settings`, {
        headers,
        data: {
          onboardingCompleted: true,
          language: "en",
          syncEnabled: false,
          autoUpdateCheckEnabled: false,
        },
      });
      expect(settings.ok()).toBe(true);
      const spending = await page.request.put(`${BASE_URL}/api/v1/spending/settings`, {
        headers,
        data: { enabled: true, accountIds: [] },
      });
      expect(spending.ok()).toBe(true);

      for (const addon of addons) {
        const root = join(addonDirectory!, addon.id);
        const manifest = JSON.parse(readFileSync(join(root, "manifest.json"), "utf8"));
        const installed = await page.request.post(`${BASE_URL}/api/v1/addons/install-zip`, {
          headers,
          data: {
            zipDataB64: readFileSync(
              join(root, "dist", `${addon.id}-${manifest.version}.zip`),
            ).toString("base64"),
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
          page.frameLocator("iframe").getByRole("button", { name: addon.upload, exact: true }),
        ).toBeVisible({ timeout: 30_000 });
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
    });
  }
}
