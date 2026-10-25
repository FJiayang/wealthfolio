import { expect, test } from "@playwright/test";
import { BASE_URL, gotoAppPath } from "./helpers";

test("removing a failed legacy benchmark restores history fetching in LTR and RTL", async ({
  page,
}) => {
  const state = await (
    await page.request.post(`${BASE_URL}/api/v1/profiles/get_profile_state`, { data: {} })
  ).json();
  const session =
    state.session ??
    (await (
      await page.request.post(`${BASE_URL}/api/v1/profiles/unlock_profile`, {
        data: { profileId: state.profiles[0].id },
      })
    ).json());
  const settings = await page.request.put(`${BASE_URL}/api/v1/settings`, {
    headers: { "x-wf-profile-scope": session.scopeId },
    data: { onboardingCompleted: true, language: "en", timezone: "UTC", baseCurrency: "USD" },
  });
  expect(settings.ok()).toBeTruthy();
  await page.goto(BASE_URL);
  for (const direction of ["ltr", "rtl"]) {
    await page.evaluate((profileId: string) => {
      localStorage.setItem(
        `profile:${profileId}:performance:selectedItems`,
        JSON.stringify([
          { id: "unknown-legacy-benchmark", type: "symbol", name: "Unknown benchmark" },
          { id: "^HSI", type: "symbol", name: "Hang Seng" },
        ]),
      );
    }, session.profileId);
    await gotoAppPath(page, `/insights?tab=performance&dir=${direction}`);
    await page.reload();
    const fetch = page.getByRole("button", { name: "Fetch / fill benchmark history" });
    await expect(page.getByRole("alert").filter({ hasText: "select" })).toBeVisible();
    await expect(fetch).toBeDisabled();
    await page
      .getByRole("button", { name: "Remove Unknown benchmark" })
      .filter({ visible: true })
      .click();
    await expect(fetch).toBeEnabled();
    await expect(page.locator("html")).toHaveAttribute("dir", direction);
  }
});
