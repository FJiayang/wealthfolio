import { expect, it } from "vitest";
import { fallbackPalette, getTreemapColor } from "./treemap-palette";

it.each(["daily", "pnl", "return"] as const)(
  "uses resolved treemap palettes for %s returns at the same intensity",
  (period) => {
    // CSS resolves the selected gain/loss roles before the snapshot reaches the chart.
    const resolvedPalette = {
      ...fallbackPalette,
      positiveLow: fallbackPalette.negativeLow,
      positiveMid: fallbackPalette.negativeMid,
      positiveHigh: fallbackPalette.negativeHigh,
      negativeLow: fallbackPalette.positiveLow,
      negativeMid: fallbackPalette.positiveMid,
      negativeHigh: fallbackPalette.positiveHigh,
    };
    const original = structuredClone(resolvedPalette);
    const gain = period === "daily" ? 0.025 : 0.5;
    const loss = period === "daily" ? -0.0125 : -0.25;
    expect(getTreemapColor(gain, period, resolvedPalette)).toEqual(getTreemapColor(loss, period));
    expect(getTreemapColor(loss, period, resolvedPalette)).toEqual(getTreemapColor(gain, period));
    expect(getTreemapColor(gain, period, fallbackPalette)).toEqual(getTreemapColor(gain, period));
    // Zero uses the selected Classic low-stop token without gain/loss intensity.
    expect(getTreemapColor(0, period, resolvedPalette).fill).toBe("rgb(250,226,218)");
    expect(getTreemapColor(0, period).fill).toBe("rgb(230,236,213)");
    expect(resolvedPalette).toEqual(original);
  },
);
