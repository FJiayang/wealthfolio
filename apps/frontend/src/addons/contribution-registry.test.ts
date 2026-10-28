import { afterEach, describe, expect, it, vi } from "vitest";
import type { AddonContributes, AddonManifest } from "@wealthfolio/addon-sdk";

// The real logger routes to the Tauri log plugin, which is unavailable under
// vitest; stub it so skipped-entry warnings don't produce unhandled rejections.
vi.mock("@/adapters", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/adapters")>();
  return {
    ...actual,
    logger: { error: vi.fn(), warn: vi.fn(), info: vi.fn(), trace: vi.fn(), debug: vi.fn() },
  };
});

import {
  clearAddonContributions,
  clearAllContributions,
  getDurableNavItems,
  getDurableRoutes,
  ingestAddonContributions,
} from "./contribution-registry";
import {
  clearAddonRegistrations,
  getDynamicNavItems,
  getDynamicRoutes,
  registerAddonNavItem,
  registerAddonRoute,
  setInstalledAddonIds,
} from "./addons-runtime-context";

function manifest(id: string, contributes: AddonContributes): AddonManifest {
  return {
    id,
    name: id,
    version: "1.0.0",
    contributes,
  };
}

const ADDON = "swingfolio-addon";

describe("contribution registry", () => {
  it("isolates import slots and scopes duplicate ids to each addon and slot", () => {
    ingestAddonContributions(
      ADDON,
      manifest(ADDON, {
        routes: [{ id: "home" }],
        links: {
          sidebar: [{ id: "same", route: "home", label: "Sidebar" }],
          "investment-import": [
            { id: "same", route: "home", label: "Investment", order: 20 },
            { id: "same", route: "home", label: "Duplicate" },
            { id: "first", route: "home", label: "First", order: 1 },
            { route: "missing", label: "Missing" },
            { id: "empty", route: "home", label: "" },
          ],
          "spending-import": [{ id: "same", route: "home", label: "Spending" }],
        },
      }),
    );
    ingestAddonContributions(
      "other-addon",
      manifest("other-addon", {
        routes: [{ id: "home" }],
        links: { "investment-import": [{ id: "same", route: "home", label: "Other" }] },
      }),
    );

    expect(getDynamicNavItems().map((item) => item.title)).toEqual(["Sidebar"]);
    expect(getDurableNavItems("investment-import").map((item) => item.title)).toEqual([
      "First",
      "Investment",
      "Other",
    ]);
    expect(getDurableNavItems("spending-import").map((item) => item.title)).toEqual(["Spending"]);
  });

  it("keeps import-only routes on runtime stop and clears links on disable or reload", () => {
    const contributions: AddonContributes = {
      routes: [{ id: "home" }],
      links: {
        "investment-import": [{ route: "home", label: "Investment" }],
        "spending-import": [{ route: "home", label: "Spending" }],
      },
    };
    ingestAddonContributions(ADDON, manifest(ADDON, contributions));
    clearAddonRegistrations(ADDON);
    expect(getDynamicNavItems()).toEqual([]);
    expect(getDynamicRoutes()).toEqual([
      expect.objectContaining({ addonId: ADDON, routeId: "home", href: `/addons/${ADDON}` }),
    ]);
    expect(getDurableNavItems("investment-import")).toHaveLength(1);
    expect(getDurableNavItems("spending-import")).toHaveLength(1);

    clearAllContributions(new Set([ADDON]));
    expect(getDurableNavItems("spending-import")).toHaveLength(1);
    clearAddonContributions(ADDON);
    expect(getDurableNavItems("investment-import")).toEqual([]);
    expect(getDurableNavItems("spending-import")).toEqual([]);
    expect(getDynamicRoutes()).toEqual([]);

    ingestAddonContributions(ADDON, manifest(ADDON, contributions));
    clearAllContributions();
    ingestAddonContributions(ADDON, manifest(ADDON, { routes: [{ id: "home" }] }));
    expect(getDurableNavItems("investment-import")).toEqual([]);
    expect(getDurableNavItems("spending-import")).toEqual([]);
  });

  afterEach(() => {
    clearAllContributions();
    clearAddonRegistrations(ADDON);
    clearAddonRegistrations("other-addon");
    setInstalledAddonIds([]);
  });

  it("ingests declared routes and sidebar links into the durable nav/route getters", () => {
    ingestAddonContributions(
      ADDON,
      manifest(ADDON, {
        routes: [{ id: "home" }],
        links: {
          sidebar: [
            { id: "home-nav", route: "home", label: "Swingfolio", icon: "trend-up", order: 5 },
          ],
        },
      }),
    );

    expect(getDurableNavItems()).toEqual([
      expect.objectContaining({
        addonId: ADDON,
        id: `${ADDON}:home-nav`,
        title: "Swingfolio",
        href: `/addons/${ADDON}`,
        icon: "trend-up",
        order: 5,
      }),
    ]);
    expect(getDurableRoutes()).toEqual([
      expect.objectContaining({
        addonId: ADDON,
        routeId: "home",
        href: `/addons/${ADDON}`,
        path: `addons/${ADDON}`,
        title: "Swingfolio",
      }),
    ]);
  });

  it("defaults a link id to its route id when omitted", () => {
    ingestAddonContributions(
      ADDON,
      manifest(ADDON, {
        routes: [{ id: "home" }],
        links: { sidebar: [{ route: "home", label: "Swingfolio" }] },
      }),
    );

    expect(getDurableNavItems().map((item) => item.id)).toEqual([`${ADDON}:home`]);
  });

  it("skips invalid and duplicate route declarations", () => {
    ingestAddonContributions(
      ADDON,
      manifest(ADDON, {
        routes: [
          { id: "good" },
          { id: "ext", path: "https://evil.example.com/x" },
          { id: "absolute", path: "/addons/swingfolio-addon" },
          { id: "parent", path: "../settings" },
          { id: "query", path: "reports?view=all" },
          { id: "encoded", path: "%2e%2e/settings" },
          { id: "", path: "empty-id" },
          { id: "same-path" },
          { id: "good", path: "duplicate-id" },
        ],
        links: { sidebar: [{ route: "good", label: "Good" }] },
      }),
    );

    const routeIds = getDurableRoutes().map((route) => route.routeId);
    expect(routeIds).toEqual(["good"]);
    // The duplicate must not overwrite the first "good" route.
    expect(getDurableRoutes()[0].href).toBe(`/addons/${ADDON}`);
    expect(getDurableNavItems().map((item) => item.id)).toEqual([`${ADDON}:good`]);
  });

  it("skips invalid sidebar links (bad route ref, empty label, duplicate id)", () => {
    ingestAddonContributions(
      ADDON,
      manifest(ADDON, {
        routes: [{ id: "home" }],
        links: {
          sidebar: [
            { route: "home", label: "Good" },
            { route: "missing", label: "Ghost" },
            { route: "home", label: "" },
            { id: "home", route: "home", label: "Duplicate" },
          ],
        },
      }),
    );

    const nav = getDurableNavItems();
    expect(nav.map((item) => item.id)).toEqual([`${ADDON}:home`]);
    // The duplicate must not overwrite the first link.
    expect(nav[0].title).toBe("Good");
    // The route itself is unaffected by the bad links.
    expect(getDurableRoutes().map((route) => route.routeId)).toEqual(["home"]);
  });

  it("creates a durable route (deep-link only) for a route with no link", () => {
    ingestAddonContributions(
      ADDON,
      manifest(ADDON, {
        routes: [{ id: "hidden", path: "hidden/:id" }],
      }),
    );

    expect(getDurableRoutes()).toEqual([
      expect.objectContaining({
        addonId: ADDON,
        routeId: "hidden",
        href: `/addons/${ADDON}/hidden/:id`,
        path: `addons/${ADDON}/hidden/:id`,
      }),
    ]);
    expect(getDurableNavItems()).toEqual([]);
  });

  it("ignores links in unknown slots (future host surfaces)", () => {
    ingestAddonContributions(
      ADDON,
      manifest(ADDON, {
        routes: [{ id: "home" }],
        links: {
          "asset/actions": [{ route: "home", label: "Open Swingfolio" }],
        },
      }),
    );

    expect(getDurableRoutes().map((route) => route.routeId)).toEqual(["home"]);
    expect(getDurableNavItems()).toEqual([]);
  });

  it("clears one addon's contributions without touching another", () => {
    ingestAddonContributions(
      ADDON,
      manifest(ADDON, {
        routes: [{ id: "home" }],
        links: { sidebar: [{ route: "home", label: "Swingfolio" }] },
      }),
    );
    ingestAddonContributions(
      "other-addon",
      manifest("other-addon", {
        routes: [{ id: "home" }],
        links: { sidebar: [{ route: "home", label: "Other" }] },
      }),
    );

    clearAddonContributions(ADDON);

    expect(getDurableNavItems().map((item) => item.addonId)).toEqual(["other-addon"]);
    expect(getDurableRoutes().map((route) => route.addonId)).toEqual(["other-addon"]);
  });

  it("mounts identical contributed suffixes under each addon's own namespace", () => {
    ingestAddonContributions(ADDON, manifest(ADDON, { routes: [{ id: "home" }] }));
    ingestAddonContributions("other-addon", manifest("other-addon", { routes: [{ id: "home" }] }));

    expect(getDurableRoutes().map((route) => route.href)).toEqual([
      "/addons/other-addon",
      `/addons/${ADDON}`,
    ]);
  });

  it("dedupes a transient runtime registration that duplicates a durable id (durable wins)", () => {
    ingestAddonContributions(
      ADDON,
      manifest(ADDON, {
        routes: [{ id: "home" }],
        links: { sidebar: [{ route: "home", label: "Durable Home" }] },
      }),
    );

    // Runtime registration reusing the same id (RFC A2: contributed route id ==
    // runtime route id; a link id defaults to its route id).
    registerAddonNavItem(ADDON, {
      id: "home",
      label: "Transient Home",
      route: `/addons/${ADDON}`,
    });
    registerAddonRoute(ADDON, { path: `/addons/${ADDON}`, routeId: "home" });

    const navForAddon = getDynamicNavItems().filter((item) => item.addonId === ADDON);
    expect(navForAddon).toHaveLength(1);
    expect(navForAddon[0].title).toBe("Durable Home");

    const routesForAddon = getDynamicRoutes().filter((route) => route.addonId === ADDON);
    expect(routesForAddon).toHaveLength(1);
    expect(routesForAddon[0].title).toBe("Durable Home");
  });
});
