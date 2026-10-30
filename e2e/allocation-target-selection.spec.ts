import { expect, test as base, type Page } from "@playwright/test";
import { BASE_URL } from "./helpers";

// A fixture gets its own teardown budget, even when the test body times out.
const test = base.extend<{ ownedProfile: { id: string; name: string } }>({
  ownedProfile: [
    async ({ context, playwright }, use, testInfo) => {
      const before = await context.request.post(`${BASE_URL}/api/v1/profiles/get_profile_state`, {
        data: {},
        timeout: 15000,
      });
      expect(before.ok(), await before.text()).toBe(true);
      const initialRegistry = (await before.json()) as { profiles: { id: string }[] };
      const existingIds = initialRegistry.profiles.map(({ id }) => id);
      const created = await context.request.post(`${BASE_URL}/api/v1/profiles/create_profile`, {
        data: { name: "Target preferences", avatarId: "clay-bot-animated" },
        timeout: 15000,
      });
      expect(created.ok(), await created.text()).toBe(true);
      const profile = (await created.json()) as { id: string; name: string };
      try {
        await use(profile);
      } finally {
        const hadTestFailure = testInfo.errors.length > 0;
        try {
          // Preserve this browser's owner cookie, but do not depend on page requests
          // surviving a timed-out test. Never select/delete an unrelated profile.
          const cleanup = await playwright.request.newContext({
            storageState: await context.request.storageState(),
            timeout: 20000,
          });
          try {
            await Promise.all(context.pages().map((page) => page.close()));
            const unlocked = await cleanup.post(`${BASE_URL}/api/v1/profiles/unlock_profile`, {
              data: { profileId: profile.id },
            });
            expect(unlocked.ok(), await unlocked.text()).toBe(true);
            const session = (await unlocked.json()) as { scopeId: string };
            const deleted = await cleanup.post(`${BASE_URL}/api/v1/profiles/delete_profile`, {
              headers: { "x-wf-profile-scope": session.scopeId },
              data: { profileId: profile.id, confirmation: profile.name },
            });
            expect(deleted.ok(), await deleted.text()).toBe(true);
            const state = await cleanup.post(`${BASE_URL}/api/v1/profiles/get_profile_state`, {
              data: {},
            });
            expect(state.ok(), await state.text()).toBe(true);
            const registry = (await state.json()) as { profiles: { id: string }[] };
            const remainingIds = registry.profiles.map(({ id }) => id);
            expect(remainingIds).not.toContain(profile.id);
            expect(remainingIds).toEqual(expect.arrayContaining(existingIds));
          } finally {
            await cleanup.dispose();
          }
        } catch (error) {
          if (!hadTestFailure) throw error;
          await testInfo.attach("owned-profile-cleanup-error", {
            body: String(error),
            contentType: "text/plain",
          });
        }
      }
    },
    { timeout: 90000 },
  ],
});

function picker(page: Page) {
  return page.getByRole("group", { name: "Select target", exact: true });
}

async function expectSelectedFirst(page: Page, name: string) {
  const selector = picker(page);
  await expect(selector.getByRole("button").first()).toHaveText(name);
  await expect(selector.getByRole("button", { name, exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect
    .poll(() =>
      selector.evaluate((element) => {
        const first = element.querySelector("button");
        if (!first) return false;
        const viewport = element.getBoundingClientRect();
        const bounds = first.getBoundingClientRect();
        // Native focus may reveal the strip's padding. The selected label's start
        // must stay visible rather than retaining the old target's scroll position.
        const start = getComputedStyle(element).direction === "rtl" ? bounds.right : bounds.left;
        return start >= viewport.left && start <= viewport.right;
      }),
    )
    .toBe(true);
}

async function changeScope(page: Page, name: string) {
  await page
    .getByRole("combobox", { name: /^Select account scope:/ })
    .first()
    .click();
  await page
    .getByRole("option", {
      name: name === "All Accounts" ? /^All Accounts$/ : new RegExp(`^${name}`),
    })
    .click();
  await page.keyboard.press("Escape");
}

test("remembers the selected target per scope and puts it first after reload, in LTR and RTL", async ({
  page,
  ownedProfile,
}) => {
  test.setTimeout(180000);

  let requestHeaders: Record<string, string> = {};

  async function post<T>(page: Page, path: string, data?: unknown): Promise<T> {
    const response = await page.request.post(`${BASE_URL}/api/v1${path}`, {
      data,
      headers: requestHeaders,
    });
    expect(response.ok(), `${path}: ${await response.text()}`).toBe(true);
    return response.json() as Promise<T>;
  }

  async function createTarget(page: Page, name: string, accountId?: string) {
    const saved = await post<{ target: { id: string; name: string } }>(
      page,
      "/allocation-targets/save-with-weights",
      {
        input: {
          name,
          scopeType: accountId ? "account" : "all",
          scopeId: accountId ?? null,
          taxonomyId: "asset_classes",
          triggerType: "manual",
          driftBandBps: 500,
          bandType: "absolute",
          relativeFactorBps: 2000,
          rebalanceGoal: "exact_target",
          minTradeAmount: "0",
          wholeSharesOnly: false,
          allowSells: true,
          maxTurnoverBps: null,
        },
        weights: [{ categoryId: "CASH", targetBps: 10000, isLocked: false, isRequired: true }],
      },
    );
    return saved.target;
  }

  const session = await post<{ scopeId: string }>(page, "/profiles/unlock_profile", {
    profileId: ownedProfile.id,
  });
  requestHeaders = { "x-wf-profile-scope": session.scopeId };
  const settings = await page.request.put(`${BASE_URL}/api/v1/settings`, {
    headers: requestHeaders,
    data: { onboardingCompleted: true, language: "en", timezone: "UTC", baseCurrency: "USD" },
  });
  expect(settings.ok(), await settings.text()).toBe(true);

  const account = await post<{ id: string }>(page, "/accounts", {
    name: "Preference brokerage",
    accountType: "SECURITIES",
    currency: "USD",
    isDefault: false,
    isActive: true,
    isArchived: false,
    trackingMode: "TRANSACTIONS",
  });
  await post(page, "/activities", {
    id: `${account.id}-deposit`,
    accountId: account.id,
    activityType: "DEPOSIT",
    activityDate: "2026-05-10T10:00:00.000Z",
    amount: 1000,
    currency: "USD",
  });
  const allA = await createTarget(page, "Global preference A: cash allocation");
  await createTarget(page, "Global preference B: cash allocation");
  const allC = await createTarget(page, "Global preference C: cash allocation");
  await createTarget(page, "Account preference A", account.id);
  const accountB = await createTarget(page, "Account preference B", account.id);
  const recalculate = await page.request.post(`${BASE_URL}/api/v1/portfolio/recalculate`, {
    headers: requestHeaders,
  });
  expect(recalculate.status()).toBe(202);

  let originalOrder: string[] = [];
  for (const dir of ["ltr", "rtl"]) {
    await page.goto(`${BASE_URL}/insights?dir=${dir}`);
    await expect(picker(page).getByRole("button")).toHaveCount(3);
    if (originalOrder.length === 0)
      originalOrder = await picker(page).getByRole("button").allTextContents();
    await picker(page).getByRole("button", { name: allA.name, exact: true }).click();
    await expectSelectedFirst(page, allA.name);
    await picker(page).getByRole("button", { name: allC.name, exact: true }).click();
    await expectSelectedFirst(page, allC.name);
    await expect(picker(page).getByRole("button")).toHaveText([
      allC.name,
      ...originalOrder.filter((name) => name !== allC.name),
    ]);
    await page.reload();
    await expectSelectedFirst(page, allC.name);
    await expect(page.locator("html")).toHaveAttribute("dir", dir);
    await page.screenshot({ path: `test-results/target-selection-${dir}.png`, fullPage: true });
  }

  await page.getByRole("button", { name: /^Details/ }).click();
  await page.getByRole("button", { name: allC.name, exact: true }).click();
  await expect(page.getByRole("menuitem").first()).toHaveText(allC.name);
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Back to allocation", exact: true }).click();

  await changeScope(page, "Preference brokerage");
  await picker(page).getByRole("button", { name: accountB.name, exact: true }).click();
  await expectSelectedFirst(page, accountB.name);
  await page.reload();
  // Account filters remain session-only; each scope still remembers its own target.
  await expectSelectedFirst(page, allC.name);
  await changeScope(page, "Preference brokerage");
  await expectSelectedFirst(page, accountB.name);
  await changeScope(page, "All Accounts");
  await expectSelectedFirst(page, allC.name);
  await changeScope(page, "Preference brokerage");
  await expectSelectedFirst(page, accountB.name);
  await changeScope(page, "All Accounts");

  const phone = await page.context().newPage();
  await phone.setViewportSize({ width: 390, height: 844 });
  await phone.goto(`${BASE_URL}/insights?dir=rtl`);
  await expectSelectedFirst(phone, allC.name);
  await picker(phone).scrollIntoViewIfNeeded();
  await expect(picker(phone).getByRole("button").first()).toBeInViewport();
  await phone.screenshot({ path: "test-results/target-selection-phone-rtl.png", fullPage: true });
  await phone.close();

  const deleted = await page.request.delete(`${BASE_URL}/api/v1/allocation-targets/${allC.id}`, {
    headers: requestHeaders,
  });
  expect(deleted.ok()).toBe(true);
  await page.reload();
  await expect(picker(page).getByRole("button", { name: allC.name, exact: true })).toHaveCount(0);
  await expect(picker(page).getByRole("button", { pressed: true })).toHaveCount(1);
  await picker(page).getByRole("button", { name: allA.name, exact: true }).click();
  await expectSelectedFirst(page, allA.name);
  const archived = await page.request.post(
    `${BASE_URL}/api/v1/allocation-targets/${allA.id}/archive`,
    { headers: requestHeaders },
  );
  expect(archived.ok()).toBe(true);
  await page.reload();
  await expect(picker(page).getByRole("button", { name: allA.name, exact: true })).toHaveCount(0);
  await expect(picker(page).getByRole("button", { pressed: true })).toHaveCount(1);
});
