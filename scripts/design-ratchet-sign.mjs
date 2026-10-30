// Counts success and destructive colours picked by a number's sign, for the design
// ratchet (design-ratchet.mjs); kept apart so its matching can be tested.
// Gains and losses of a price or value use the gain and loss roles (text-gain,
// bg-loss-subtle, fill-gain, var(--loss)), which the Gain and loss colours setting
// swaps for markets where red means up; success and destructive stay for status. A
// success or destructive colour counts when the other one and a sign test (a
// comparison with zero, Math.sign, positive or negative, isGain, isLoss, isUp,
// isDown, "up" or "down") sit within six lines of it. Status pairs with a sign test
// (budgets, goals, spending) count too; they are recorded, and only a new one fails.
// So does every reference to the treemap's --heatmap-positive-* and
// --heatmap-negative-* ramps, which only globals.css reads, behind the swap.
const statusColor =
  /(?<![\w-])(?:(?:text|bg|fill|stroke|border|ring|from|to|via|decoration|outline)-(success|destructive)(?:-subtle(?:-foreground|-border)?)?(?:\/\d+)?(?![\w-])|var\(--(success|destructive)(?:-subtle(?:-foreground|-border)?)?\))/g;
const signTest =
  /[<>]=?\s*-?0(?:\.\d+)?(?![\w.])|(?<![\w.])-?0(?:\.\d+)?\s*[<>]|Math\.sign|positive|negative|\bis(?:Gain|Loss|Up|Down)\b|"(?:up|down)"/i;
const heatmapRamp = /heatmap-(?:positive|negative)-/g;
const reach = 6;

// These three fork balance statuses were preserved when gain/loss colours were
// added (483aa3e2).
// Debt balances and net worth keep their solvency/status meaning when market
// gain colours switch. This is not permission to colour investment returns with
// status roles: only each exact expression at its owning path is exempt, once.
const balanceStatuses = {
  "apps/frontend/src/pages/holdings/components/holdings-grouped-table.tsx": [
    /className=\{cn\(\s*"font-medium",\s*group\.name === "Liabilities" &&\s*\(group\.totalValue >= 0 \? "text-success" : "text-destructive"\),\s*\)\}/g,
    /className=\{\s*holding\.isLiability\s*\? displayValue >= 0\s*\? "text-success"\s*: "text-destructive"\s*: undefined\s*\}/g,
  ],
  "apps/frontend/src/pages/holdings/components/holdings-summary-bar.tsx": [
    /className=\{netWorth >= 0 \? "text-success" : "text-destructive"\}/g,
  ],
};

export function signColors(source, path = "") {
  const exemptRanges = (balanceStatuses[path] ?? []).flatMap((expression) => {
    const matches = [...source.matchAll(expression)];
    // A duplicate is new debt, not another approved use.
    return matches.length === 1
      ? [[matches[0].index, matches[0].index + matches[0][0].length]]
      : [];
  });
  const lines = source.split("\n");
  let offset = 0;
  const colors = lines.map((line) => {
    const tokens = [...line.matchAll(statusColor)].map((m) => ({
      color: m[1] ?? m[2],
      exempt: exemptRanges.some(
        ([start, end]) => offset + m.index >= start && offset + m.index < end,
      ),
    }));
    offset += line.length + 1;
    return tokens;
  });
  let count = [...source.matchAll(heatmapRamp)].length;
  lines.forEach((_, index) => {
    const start = Math.max(0, index - reach);
    const end = index + reach + 1;
    const nearby = colors
      .slice(start, end)
      .flat()
      .map((token) => token.color);
    const signed = signTest.test(lines.slice(start, end).join("\n"));
    for (const { color, exempt } of colors[index]) {
      if (exempt) continue;
      const other = color === "success" ? "destructive" : "success";
      if (signed && nearby.includes(other)) count += 1;
    }
  });
  return count;
}
