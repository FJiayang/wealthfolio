import { expect, test as base } from "@playwright/test";
import { BASE_URL } from "./helpers";

// Keep data-size and navigation checks independent of earlier serial specs.
export const test = base.extend<{ isolatedProfile: void }>({
  isolatedProfile: [
    async ({ context, playwright }, use, testInfo) => {
      const before = await context.request.post(`${BASE_URL}/api/v1/profiles/get_profile_state`, {
        data: {},
      });
      expect(before.ok(), await before.text()).toBe(true);
      const existingIds = ((await before.json()) as { profiles: { id: string }[] }).profiles.map(
        ({ id }) => id,
      );
      const created = await context.request.post(`${BASE_URL}/api/v1/profiles/create_profile`, {
        data: { name: "Isolated browser regression", avatarId: "clay-bot-animated" },
      });
      expect(created.ok(), await created.text()).toBe(true);
      const profile = (await created.json()) as { id: string; name: string };
      try {
        const unlocked = await context.request.post(`${BASE_URL}/api/v1/profiles/unlock_profile`, {
          data: { profileId: profile.id },
        });
        expect(unlocked.ok(), await unlocked.text()).toBe(true);
        const session = (await unlocked.json()) as { scopeId: string };
        await context.setExtraHTTPHeaders({ "x-wf-profile-scope": session.scopeId });
        const settings = await context.request.put(`${BASE_URL}/api/v1/settings`, {
          data: {
            onboardingCompleted: true,
            language: "en",
            timezone: "UTC",
            baseCurrency: "USD",
            autoUpdateCheckEnabled: false,
          },
        });
        expect(settings.ok(), await settings.text()).toBe(true);
        await use();
      } finally {
        const hadTestFailure = testInfo.errors.length > 0;
        try {
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
            const remaining = ((await state.json()) as { profiles: { id: string }[] }).profiles.map(
              ({ id }) => id,
            );
            expect(remaining).not.toContain(profile.id);
            expect(remaining).toEqual(expect.arrayContaining(existingIds));
          } finally {
            await cleanup.dispose();
          }
        } catch (error) {
          if (!hadTestFailure) throw error;
          await testInfo.attach("isolated-profile-cleanup-error", {
            body: String(error),
            contentType: "text/plain",
          });
        }
      }
    },
    { auto: true, timeout: 90000 },
  ],
});
