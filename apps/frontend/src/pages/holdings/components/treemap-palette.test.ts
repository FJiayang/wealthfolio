import { describe, expect, it } from "vitest";
import { fallbackPalette, getTreemapColor } from "./treemap-palette";

describe("treemap palette", () => {
  it("keeps Classic's ramp: the tile colours before themes", () => {
    // +50% total return is the gain ramp's mid-tone (k = 0.5): halfway from
    // #cdd9bf to #355c4c. -25% is the loss mid-tone: halfway from #e9b3a8 to #d14e42.
    expect(getTreemapColor(0.5, "pnl")).toEqual({ fill: "rgb(129,155,134)", isLightTile: false });
    expect(getTreemapColor(-0.25, "pnl")).toEqual({ fill: "rgb(221,129,117)", isLightTile: true });
    expect(getTreemapColor(0, "daily").fill).toBe("rgb(205,217,191)");
  });

  it("draws the theme's ramp", () => {
    const palette = {
      ...fallbackPalette,
      positiveLow: [0, 0, 0],
      positiveHigh: [200, 100, 0],
    };
    expect(getTreemapColor(0.5, "pnl", palette).fill).toBe("rgb(100,50,0)");
  });

  it.each(["daily", "pnl", "return"] as const)(
    "swaps custom theme ramps without changing %s return intensity or mutating the palette",
    (period) => {
      const palette = {
        ...fallbackPalette,
        positiveLow: [10, 20, 30],
        positiveHigh: [110, 120, 130],
        negativeLow: [200, 210, 220],
        negativeHigh: [100, 110, 120],
      };
      const original = structuredClone(palette);
      const gain = period === "daily" ? 0.025 : 0.5;
      const loss = period === "daily" ? -0.0125 : -0.25;

      expect(getTreemapColor(gain, period, palette, true)).toEqual({
        fill: "rgb(150,160,170)",
        isLightTile: true,
      });
      expect(getTreemapColor(loss, period, palette, true)).toEqual({
        fill: "rgb(60,70,80)",
        isLightTile: false,
      });
      expect(getTreemapColor(gain, period, palette, false).fill).toBe("rgb(60,70,80)");
      expect(getTreemapColor(loss, period, palette, false).fill).toBe("rgb(150,160,170)");
      expect(getTreemapColor(0, period, palette, true).fill).toBe("rgb(200,210,220)");
      expect(getTreemapColor(NaN, period, palette, true).fill).toBe("rgb(200,210,220)");
      expect(palette).toEqual(original);
    },
  );
});
