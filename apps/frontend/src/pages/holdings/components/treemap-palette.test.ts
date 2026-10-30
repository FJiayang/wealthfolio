import { describe, expect, it, vi } from "vitest";
import { fallbackPalette, getTreemapColor, subscribeToTreemapTheme } from "./treemap-palette";

describe("treemap palette", () => {
  it("draws Classic's ramp: soft sage and rose through their midpoints", () => {
    // +50% total return is the gain ramp's middle (k = 0.5), #b8c496; -25% is the loss
    // ramp's middle, #dca89d. A flat day is the low stop.
    expect(getTreemapColor(0.5, "pnl")).toEqual({ fill: "rgb(184,196,150)", isLightTile: true });
    expect(getTreemapColor(-0.25, "pnl")).toEqual({ fill: "rgb(220,168,157)", isLightTile: true });
    expect(getTreemapColor(0, "daily").fill).toBe("rgb(230,236,213)");
    // -5% on the day is 60% of the way from the middle to #bd6f60; ink reads best on it.
    expect(getTreemapColor(-0.05, "daily")).toEqual({
      fill: "rgb(201,134,120)",
      isLightTile: true,
    });
  });

  it("draws the theme's ramp through its middle stop", () => {
    const palette = {
      ...fallbackPalette,
      positiveLow: [0, 0, 0],
      positiveMid: [100, 50, 0],
      positiveHigh: [200, 200, 200],
    };
    expect(getTreemapColor(0.5 / 3, "pnl", palette).fill).toBe("rgb(50,25,0)");
    expect(getTreemapColor(0.5, "pnl", palette).fill).toBe("rgb(100,50,0)");
    expect(getTreemapColor(1.5, "pnl", palette).fill).toBe("rgb(150,125,100)");
  });

  it("keeps a straight ramp when the middle stop is halfway", () => {
    // The other themes set their middle stop halfway between low and high,
    // so their tiles stay where a straight blend from low to high put them.
    const low = [58, 31, 29];
    const high = [255, 69, 58];
    const palette = {
      ...fallbackPalette,
      negativeLow: low,
      negativeMid: low.map((v, i) => Math.round((v + high[i]) / 2)),
      negativeHigh: high,
    };
    for (const move of [0.001, 0.0025, 0.005, 0.01, 0.0125, 0.02, 0.05, 0.2]) {
      const t = move / (move + 0.0125);
      const straight = low.map((v, i) => v + (high[i] - v) * t);
      const fill = getTreemapColor(-move, "daily", palette).fill.match(/\d+/g)!.map(Number);
      fill.forEach((v, i) => expect(Math.abs(v - straight[i])).toBeLessThanOrEqual(1));
    }
  });

  it("labels each tile with the label that contrasts more", () => {
    // A mid red tile, #e36a5a: ink reads at 5.9:1, paper at 3.2:1, though the tile
    // is darker than a luminance cut-off would call light.
    const tile = [227, 106, 90];
    const palette = {
      ...fallbackPalette,
      negativeLow: tile,
      negativeMid: tile,
      negativeHigh: tile,
    };
    expect(getTreemapColor(-0.015, "daily", palette).isLightTile).toBe(true);
  });

  it("re-reads the palette when the mode, theme or gain and loss colours change", async () => {
    const onChange = vi.fn();
    const unsubscribe = subscribeToTreemapTheme(onChange);
    const root = document.documentElement;
    for (const change of [
      () => root.setAttribute("data-gain-colors", "red-up"),
      () => root.setAttribute("data-theme", "ember"),
      () => root.classList.add("dark"),
    ]) {
      onChange.mockClear();
      change();
      await Promise.resolve();
      expect(onChange).toHaveBeenCalled();
    }
    unsubscribe();
    root.removeAttribute("data-gain-colors");
    root.removeAttribute("data-theme");
    root.classList.remove("dark");
  });
  it.each(["daily", "pnl", "return"] as const)(
    "uses resolved custom theme ramps without changing %s return intensity or mutating the palette",
    (period) => {
      const palette = {
        ...fallbackPalette,
        positiveLow: [10, 20, 30],
        positiveMid: [60, 70, 80],
        positiveHigh: [110, 120, 130],
        negativeLow: [200, 210, 220],
        negativeMid: [150, 160, 170],
        negativeHigh: [100, 110, 120],
      };
      const original = structuredClone(palette);
      // The upstream CSS gain/loss roles have already resolved the user's preference.
      // Model that snapshot; getTreemapColor must not swap it a second time.
      const resolvedPalette = {
        ...palette,
        positiveLow: palette.negativeLow,
        positiveMid: palette.negativeMid,
        positiveHigh: palette.negativeHigh,
        negativeLow: palette.positiveLow,
        negativeMid: palette.positiveMid,
        negativeHigh: palette.positiveHigh,
      };
      const originalResolved = structuredClone(resolvedPalette);
      const gain = period === "daily" ? 0.025 : 0.5;
      const loss = period === "daily" ? -0.0125 : -0.25;

      expect(getTreemapColor(gain, period, resolvedPalette)).toEqual({
        fill: "rgb(150,160,170)",
        isLightTile: true,
      });
      expect(getTreemapColor(loss, period, resolvedPalette)).toEqual({
        fill: "rgb(60,70,80)",
        isLightTile: false,
      });
      expect(getTreemapColor(gain, period, palette).fill).toBe("rgb(60,70,80)");
      expect(getTreemapColor(loss, period, palette).fill).toBe("rgb(150,160,170)");
      expect(getTreemapColor(0, period, resolvedPalette).fill).toBe("rgb(200,210,220)");
      expect(getTreemapColor(NaN, period, resolvedPalette).fill).toBe("rgb(200,210,220)");
      expect(palette).toEqual(original);
      expect(resolvedPalette).toEqual(originalResolved);
    },
  );

  it("uses every resolved nonlinear ramp stop while keeping gain and loss intensity scales", () => {
    const palette = {
      ...fallbackPalette,
      positiveLow: [10, 20, 30],
      positiveMid: [30, 100, 170],
      positiveHigh: [210, 220, 230],
      negativeLow: [200, 210, 220],
      negativeMid: [120, 40, 100],
      negativeHigh: [20, 30, 40],
    };
    const original = structuredClone(palette);
    // The upstream CSS gain/loss roles have already resolved the user's preference.
    // Model that snapshot; getTreemapColor must not swap it a second time.
    const resolvedPalette = {
      ...palette,
      positiveLow: palette.negativeLow,
      positiveMid: palette.negativeMid,
      positiveHigh: palette.negativeHigh,
      negativeLow: palette.positiveLow,
      negativeMid: palette.positiveMid,
      negativeHigh: palette.positiveHigh,
    };
    const originalResolved = structuredClone(resolvedPalette);

    // Gains keep k = 0.5 even on the negative ramp: t = 0.25, 0.5, 0.75.
    expect(getTreemapColor(0.5 / 3, "pnl", resolvedPalette).fill).toBe("rgb(160,125,160)");
    expect(getTreemapColor(0.5, "pnl", resolvedPalette).fill).toBe("rgb(120,40,100)");
    expect(getTreemapColor(1.5, "pnl", resolvedPalette).fill).toBe("rgb(70,35,70)");

    // Losses keep k = 0.25 on the positive ramp, not the gain ramp's k = 0.5.
    expect(getTreemapColor(-0.25 / 3, "pnl", resolvedPalette).fill).toBe("rgb(20,60,100)");
    expect(getTreemapColor(-0.25, "pnl", resolvedPalette).fill).toBe("rgb(30,100,170)");
    expect(getTreemapColor(-0.75, "pnl", resolvedPalette).fill).toBe("rgb(120,160,200)");
    expect(palette).toEqual(original);
    expect(resolvedPalette).toEqual(originalResolved);
  });
});
