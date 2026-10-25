import { expect, test, type Page } from "@playwright/test";
import { completeOnboardingIfNeeded, gotoAppPath } from "./helpers";

async function saveSchedule(page: Page) {
  const saved = page.waitForResponse(
    (response) =>
      response.request().method() === "PUT" &&
      new URL(response.url()).pathname.endsWith("/settings"),
  );
  await page
    .getByTestId("market-data-sync-settings")
    .getByRole("button", { name: "Save", exact: true })
    .click();
  const response = await saved;
  expect(response.status(), await response.text()).toBe(200);
  return response;
}

async function selectMode(page: Page, mode: "Hourly interval" | "Daily at a fixed time") {
  await page.getByRole("combobox", { name: "Automatic sync mode" }).click();
  await page.getByRole("option", { name: mode, exact: true }).click();
}

test("scheduled market sync modes persist without losing inactive settings in LTR and RTL", async ({
  page,
}) => {
  test.setTimeout(120000);
  await completeOnboardingIfNeeded(page);

  for (const [direction, hours, time, timezone] of [
    ["ltr", 12, "09:30", "America/New_York"],
    ["rtl", 24, "17:45", "Asia/Shanghai"],
  ] as const) {
    await gotoAppPath(page, `/settings/market-data?dir=${direction}`);
    await expect(page.locator("html")).toHaveAttribute("dir", direction);
    await expect(page.getByRole("combobox", { name: "Automatic sync mode" })).toHaveText(
      "Hourly interval",
    );
    const interval = page.getByRole("combobox", { name: "Automatic sync interval" });
    await interval.click();
    await page.getByRole("option", { name: `Every ${hours} hours`, exact: true }).click();
    const intervalResponse = await saveSchedule(page);
    expect(intervalResponse.request().postDataJSON()).toMatchObject({
      marketDataSyncMode: "interval",
      marketDataSyncIntervalHours: hours,
    });
    expect(await intervalResponse.json()).toMatchObject({
      marketDataSyncMode: "interval",
      marketDataSyncIntervalHours: hours,
    });
    await page.reload();
    await expect(interval).toHaveText(`Every ${hours} hours`);

    await selectMode(page, "Daily at a fixed time");
    const dailyTime = page.getByLabel("Daily sync time", { exact: true });
    const timezoneControl = page.getByRole("combobox", { name: "Sync timezone" });
    const save = page
      .getByTestId("market-data-sync-settings")
      .getByRole("button", { name: "Save", exact: true });
    if (direction === "ltr") {
      // First daily activation never guesses a time or infers a market close.
      await expect(dailyTime).toHaveValue("");
      await expect(save).toBeDisabled();
      const deviceTimezone = await page.evaluate(
        () => Intl.DateTimeFormat().resolvedOptions().timeZone,
      );
      await expect(timezoneControl).toHaveText(deviceTimezone);
    } else {
      // Switching modes and reloading did not discard the previous daily pair.
      await expect(dailyTime).toHaveValue("09:30");
      await expect(timezoneControl).toHaveText("America/New_York");
    }
    await dailyTime.fill(time);
    await timezoneControl.click();
    await page.getByRole("option", { name: timezone, exact: true }).click();
    const dailyResponse = await saveSchedule(page);
    const dailySettings = {
      marketDataSyncMode: "daily",
      marketDataSyncIntervalHours: hours,
      marketDataSyncTime: time,
      marketDataSyncTimezone: timezone,
    };
    expect(dailyResponse.request().postDataJSON()).toEqual(dailySettings);
    expect(await dailyResponse.json()).toMatchObject(dailySettings);
    await page.reload();
    await expect(page.getByRole("combobox", { name: "Automatic sync mode" })).toHaveText(
      "Daily at a fixed time",
    );
    await expect(dailyTime).toHaveValue(time);
    await expect(dailyTime).toHaveAttribute("dir", "ltr");
    await expect(timezoneControl).toHaveText(timezone);
    for (const control of [dailyTime, timezoneControl]) {
      const bounds = await control.boundingBox();
      expect(bounds).not.toBeNull();
      expect(bounds!.x).toBeGreaterThanOrEqual(0);
      expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(page.viewportSize()!.width);
    }
    // Return to interval without deleting the daily pair or changing its hours.
    await selectMode(page, "Hourly interval");
    await expect(interval).toHaveText(`Every ${hours} hours`);
    const restored = await saveSchedule(page);
    expect(await restored.json()).toMatchObject({
      ...dailySettings,
      marketDataSyncMode: "interval",
    });
    await page.reload();
    await expect(interval).toHaveText(`Every ${hours} hours`);
  }
});
