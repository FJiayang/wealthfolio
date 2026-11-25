import { expect } from "./fixtures";
import { test } from "./isolated-profile-fixture";
import { BASE_URL, gotoAppPath } from "./helpers";

test("removing a failed legacy benchmark restores history fetching in LTR and RTL", async ({
  page,
}) => {
  test.setTimeout(60_000);
  const state = await (
    await page.request.post(`${BASE_URL}/api/v1/profiles/get_profile_state`, { data: {} })
  ).json();
  expect(state.session?.profileId).toBeTruthy();
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
    }, state.session.profileId);
    await gotoAppPath(page, `/insights?tab=performance&dir=${direction}`);
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
