import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { signColors } from "./design-ratchet-sign.mjs";

test("counts success and destructive picked by a sign", () => {
  assert.equal(
    signColors(`<span className={value >= 0 ? "text-success" : "text-destructive"} />`),
    2,
  );
  assert.equal(signColors(`const fill = isGain ? "fill-success" : "fill-destructive";`), 2);
  assert.equal(signColors(`stroke={total < -0.005 ? "var(--destructive)" : "var(--success)"}`), 2);
  assert.equal(
    signColors(`if (tone === "up") return "text-destructive";\nreturn "text-success";`),
    2,
  );
  // GainPercent's shape: the classes are picked first, the sign test comes later.
  const picked = [
    `const gainColor = invertColor ? "text-destructive" : "text-success";`,
    `const badge = "bg-success-subtle text-success-subtle-foreground";`,
    ...Array(3).fill(""),
    `className={displayValue > 0 ? gainColor : "text-foreground"}`,
  ].join("\n");
  assert.equal(signColors(picked), 4);
});

test("counts the treemap's raw ramps", () => {
  assert.equal(
    signColors(`positiveLow: "heatmap-positive-low", negativeLow: "heatmap-negative-low",`),
    2,
  );
});

test("leaves gain and loss roles and lone statuses alone", () => {
  assert.equal(signColors(`<span className={value >= 0 ? "text-gain" : "text-loss"} />`), 0);
  assert.equal(signColors(`{errors.length > 0 && <p className="text-destructive">{error}</p>}`), 0);
  assert.equal(signColors(`const fill = isGain ? "fill-gain" : "var(--loss)";`), 0);
});

test("leaves status pairs without a sign test alone", () => {
  const markers = [
    `if (variant === "buy") return "success";`,
    `case "success": return "text-success";`,
    `case "destructive": return "text-destructive";`,
  ].join("\n");
  assert.equal(signColors(markers), 0);
  // Far apart: more than six lines between the pair and the sign test.
  const apart = [
    `const ok = count >= 0;`,
    ...Array(7).fill(""),
    `ok ? "text-success" : "text-destructive"`,
  ];
  assert.equal(signColors(apart.join("\n")), 0);
});

const groupedPath = "apps/frontend/src/pages/holdings/components/holdings-grouped-table.tsx";
const summaryPath = "apps/frontend/src/pages/holdings/components/holdings-summary-bar.tsx";
const balanceExpression = 'className={netWorth >= 0 ? "text-success" : "text-destructive"}';

test("exempts only the three established semantic balance expressions at their owning paths", () => {
  for (const [path, rawCount] of [
    [groupedPath, 4],
    [summaryPath, 2],
  ]) {
    const source = readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
    assert.equal(signColors(source), rawCount);
    assert.equal(signColors(source, path), 0);
    assert.equal(signColors(source, "apps/frontend/src/other.tsx"), rawCount);
    assert.equal(signColors(source.replaceAll(">= 0", "> 0"), path), rawCount);
    assert.equal(
      signColors(source.replaceAll('"text-destructive"', '"text-destructive" + suffix'), path),
      rawCount,
    );
    assert.equal(signColors(`${source}\n${source}`, path), rawCount * 2);
  }
});

test("does not exempt changed or duplicate balance expressions", () => {
  assert.equal(signColors(balanceExpression, summaryPath), 0);
  for (const changed of [
    balanceExpression.replace(">= 0", "> 0"),
    balanceExpression.replace("netWorth", "gain"),
    balanceExpression.replace("netWorth", "other.netWorth"),
    balanceExpression.replace("text-success", "text-success-subtle"),
    balanceExpression.replace('"text-destructive"}', '"text-destructive" + suffix}'),
  ]) {
    assert.equal(signColors(changed, summaryPath), 2, changed);
  }
  assert.equal(signColors(`${balanceExpression}\n${balanceExpression}`, summaryPath), 4);
});

test("keeps original context so adjacent non-exempt status tokens still count", () => {
  const source = `${balanceExpression}\nconst gainClass = "text-success";`;
  // The original sign and destructive token must remain visible to the scanner.
  assert.equal(signColors(source, summaryPath), 1);
  assert.equal(
    signColors(
      `${balanceExpression}\nvalue >= 0 ? "text-success" : "text-destructive"`,
      summaryPath,
    ),
    2,
  );
  const separate = `${balanceExpression}\n${"\n".repeat(7)}const status = "text-success";`;
  assert.equal(signColors(separate, summaryPath), 0);
});
