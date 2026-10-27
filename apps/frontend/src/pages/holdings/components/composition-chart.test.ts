import { expect, it } from "vitest";
import { getTreemapColor } from "./treemap-palette";

it.each(["daily", "pnl", "return"] as const)(
  "swaps treemap palettes for %s returns at the same intensity",
  (period) => {
    const gain = period === "daily" ? 0.025 : 0.5;
    const loss = period === "daily" ? -0.0125 : -0.25;
    expect(getTreemapColor(gain, period, undefined, true)).toEqual(getTreemapColor(loss, period));
    expect(getTreemapColor(loss, period, undefined, true)).toEqual(getTreemapColor(gain, period));
    expect(getTreemapColor(gain, period, undefined, false)).toEqual(getTreemapColor(gain, period));
    // Zero uses the selected Classic low-stop token without gain/loss intensity.
    expect(getTreemapColor(0, period, undefined, true).fill).toBe("rgb(250,226,218)");
    expect(getTreemapColor(0, period).fill).toBe("rgb(230,236,213)");
  },
);
