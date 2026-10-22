import { expect, it } from "vitest";
import { getTreemapColor } from "./composition-chart";

it.each(["daily", "pnl", "return"] as const)(
  "swaps treemap palettes for %s returns at the same intensity",
  (period) => {
    const gain = period === "daily" ? 0.025 : 0.5;
    const loss = period === "daily" ? -0.0125 : -0.25;
    expect(getTreemapColor(gain, period, true)).toEqual(getTreemapColor(loss, period));
    expect(getTreemapColor(loss, period, true)).toEqual(getTreemapColor(gain, period));
    expect(getTreemapColor(gain, period, false)).toEqual(getTreemapColor(gain, period));
    expect(getTreemapColor(0, period, true).fill).toBe("rgb(233,179,168)");
    expect(getTreemapColor(0, period).fill).toBe("rgb(205,217,191)");
  },
);
