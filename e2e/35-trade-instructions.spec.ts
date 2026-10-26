import { randomUUID } from "node:crypto";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { BASE_URL, loginIfNeeded, selectAccountOption } from "./helpers";

// Exercise application-timezone entry from a browser in a different timezone.
test.use({ timezoneId: "America/New_York" });

async function fillInstructionDate(page: Page, picker: Locator, day: string) {
  for (const [segment, value] of [
    ["month", "01"],
    ["day", day],
    ["year", "2020"],
    ["hour", "10"],
    ["minute", "30"],
  ]) {
    await picker.locator(`[data-type="${segment}"]`).click();
    await page.keyboard.type(value);
  }
  const period = picker.locator('[data-type="dayPeriod"]');
  if (await period.count()) {
    await period.click();
    await page.keyboard.type("a");
  }
  await page.keyboard.press("Tab");
}

for (const rtl of [false, true]) {
  test(`trade instructions: create, complete and correct (${rtl ? "RTL" : "LTR"})`, async ({
    page,
  }) => {
    test.setTimeout(180_000);
    await loginIfNeeded(page);
    const api = `${BASE_URL}/api/v1`;
    const settingsResponse = await page.request.put(`${api}/settings`, {
      data: { timezone: "Asia/Shanghai" },
    });
    expect(settingsResponse.ok()).toBeTruthy();
    const suffix = Date.now().toString();
    const name = `Instructions ${suffix}`;
    const symbol = `TI${suffix}`;
    const goalResponse = await page.request.post(`${api}/goals`, {
      data: {
        title: `Goal ${suffix}`,
        goalType: "custom_save_up",
        targetAmount: 1000,
        currency: "USD",
      },
    });
    expect(goalResponse.ok()).toBeTruthy();
    const goal = await goalResponse.json();
    const accountResponse = await page.request.post(`${api}/accounts`, {
      data: {
        name,
        accountType: "SECURITIES",
        currency: "USD",
        isDefault: false,
        isActive: true,
        isArchived: false,
        trackingMode: "TRANSACTIONS",
      },
    });
    expect(accountResponse.ok()).toBeTruthy();
    const account = await accountResponse.json();
    const assetResponse = await page.request.post(`${api}/assets`, {
      data: {
        kind: "INVESTMENT",
        name: symbol,
        displayCode: symbol,
        instrumentSymbol: symbol,
        instrumentType: "EQUITY",
        quoteMode: "MANUAL",
        quoteCcy: "USD",
      },
    });
    expect(assetResponse.ok()).toBeTruthy();
    const asset = await assetResponse.json();
    await page.goto(`${BASE_URL}/goals/${goal.id}${rtl ? "?dir=rtl" : ""}`);
    await page.getByRole("button", { name: "New instruction", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: /^(New instruction|Instruction details)$/ });
    await expect(dialog.getByRole("textbox", { name: "Quantity", exact: true })).toHaveCount(0);
    await expect(dialog.getByLabel("Goal", { exact: true })).toHaveCount(0);
    await expect(page.locator('a[href="/trade-instructions"]')).toHaveCount(0);
    await page.screenshot({ path: test.info().outputPath("instruction-type-picker.png") });
    await dialog.getByTestId("activity-type-sell").click();
    await expect(dialog.getByRole("button", { name: "Add Sell", exact: true })).toBeVisible();
    await dialog.getByTestId("activity-type-buy").click();
    await selectAccountOption(page, name, "USD", dialog.getByTestId("account-select"));
    await dialog
      .getByRole("combobox")
      .filter({ hasText: /Select symbol/i })
      .click();
    await page.getByPlaceholder("Search for symbol").fill(symbol);
    const symbolOption = page
      .getByRole("option")
      .filter({ hasText: symbol, hasNotText: /Create custom/i })
      .first();
    await expect(symbolOption).toBeVisible();
    await symbolOption.click();
    await dialog.getByRole("textbox", { name: "Quantity", exact: true }).fill("2.123456789");
    const expectedPicker = dialog.getByTestId("instruction-expected-picker");
    await expectedPicker.getByRole("button", { name: "Pick a date" }).click();
    await expect(page.getByRole("grid")).toBeVisible();
    await page.screenshot({ path: test.info().outputPath("instruction-calendar.png") });
    await page.keyboard.press("Escape");
    await fillInstructionDate(page, expectedPicker, "01");
    await dialog.getByLabel("Limit price", { exact: false }).fill("12.34");
    await dialog.getByRole("button", { name: "Advanced & notes" }).click();
    await dialog.getByTestId("notes-input").fill("Optional instruction note\nSecond line");
    await expect(dialog.getByText("Research and execution notes", { exact: true })).toHaveCount(0);
    await page.screenshot({ path: test.info().outputPath("instruction-form.png") });
    await dialog.getByRole("button", { name: "Add Buy", exact: true }).click();
    await expect(dialog).toBeHidden();
    const item = page
      .getByRole("button")
      .filter({ hasText: symbol })
      .filter({ hasText: "Overdue" });
    await expect(item).toBeVisible();
    await item.click();
    await dialog.getByRole("button", { name: "Mark complete", exact: true }).click();
    await dialog.getByLabel("Resolution note").fill("Partially executed, remainder abandoned");
    await dialog.getByRole("button", { name: "Confirm", exact: true }).click();
    await expect(
      dialog.getByRole("button", { name: "Correct completion time", exact: true }),
    ).toBeVisible();
    const listing = await page.request.post(`${api}/trade-instructions/list`, {
      data: { statuses: [], goalId: goal.id },
    });
    const row = (await listing.json()).items[0];
    expect(row.goalId).toBe(goal.id);
    expect(Date.parse(row.expectedCompletionAt)).toBe(Date.parse("2020-01-01T02:30:00Z"));
    expect(row.notes).toBe("Optional instruction note\nSecond line");
    expect(row.status).toBe("COMPLETED");
    expect(row.quantity).toBe("2.123456789");
    expect(row.completedAt).toBeTruthy();
    await dialog.getByRole("button", { name: "Correct completion time", exact: true }).click();
    await fillInstructionDate(page, dialog.getByTestId("instruction-completed-picker"), "02");
    await dialog.getByRole("button", { name: "Confirm", exact: true }).click();
    await expect(
      dialog.getByRole("button", { name: "Correct completion time", exact: true }),
    ).toBeVisible();
    const corrected = await (await page.request.get(`${api}/trade-instructions/${row.id}`)).json();
    expect(Date.parse(corrected.completedAt)).toBe(Date.parse("2020-01-02T02:30:00Z"));
    expect(corrected.closedAt).toBe(row.closedAt);
    expect(corrected.version).toBe(row.version + 1);
    const activities = await (
      await page.request.post(`${api}/activities/search`, {
        data: { page: 0, pageSize: 100, accountIdFilter: [account.id] },
      })
    ).json();
    expect(activities.data).toHaveLength(0);
    const retryRequest = {
      goalId: goal.id,
      requestId: randomUUID(),
      accountId: account.id,
      asset: { id: asset.id },
      side: "BUY",
      amount: "123.45",
      amountCurrency: "USD",
    };
    const external = await page.request.post(`${api}/trade-instructions`, { data: retryRequest });
    expect(external.ok()).toBeTruthy();
    const externalRow = await external.json();
    expect(
      (await (await page.request.post(`${api}/trade-instructions`, { data: retryRequest })).json())
        .id,
    ).toBe(externalRow.id);
    const otherGoal = await (
      await page.request.post(`${api}/goals`, {
        data: { title: "Other goal", goalType: "custom_save_up", targetAmount: 2000 },
      })
    ).json();
    await page.goto(`${BASE_URL}/goals/${otherGoal.id}`);
    await expect(
      page.getByTestId("goal-trade-instructions").getByText("No instructions", { exact: true }),
    ).toBeVisible();
    await page.request.delete(`${api}/goals/${goal.id}`);
    const afterDelete = await (
      await page.request.post(`${api}/trade-instructions/list`, {
        data: { statuses: [], goalId: goal.id },
      })
    ).json();
    expect(afterDelete.total).toBe(0);
  });
}

test("MCP can create and read, but cannot complete an instruction", async ({ page }) => {
  test.setTimeout(120_000);
  await loginIfNeeded(page);
  const api = `${BASE_URL}/api/v1`;
  const account = await (
    await page.request.post(`${api}/accounts`, {
      data: {
        name: "MCP instructions",
        accountType: "SECURITIES",
        currency: "USD",
        isDefault: false,
        isActive: true,
        isArchived: false,
        trackingMode: "TRANSACTIONS",
      },
    })
  ).json();
  const goal = await (
    await page.request.post(`${api}/goals`, {
      data: { title: "MCP goal", goalType: "custom_save_up", targetAmount: 1000 },
    })
  ).json();
  const credentials = await page.request.post(`${api}/agent-access/tokens`, {
    data: {
      name: "Instruction E2E",
      scopes: [
        "trade-instructions:read",
        "trade-instructions:create",
        "accounts:read",
        "assets:write",
        "financial-planning:read",
      ],
    },
  });
  expect(credentials.ok()).toBeTruthy();
  const { token, id: tokenId } = await credentials.json();
  const url = `${process.env.WF_E2E_BACKEND_URL || "http://localhost:8088"}/mcp`;
  let session: string | undefined;
  let rpcId = 0;
  const rpc = async (method: string, params: unknown) => {
    const response = await page.request.post(url, {
      headers: {
        Authorization: `Bearer ${token}`,
        Accept: "application/json, text/event-stream",
        "MCP-Protocol-Version": "2025-03-26",
        ...(session ? { "Mcp-Session-Id": session } : {}),
      },
      data: { jsonrpc: "2.0", id: ++rpcId, method, params },
    });
    expect(response.ok()).toBeTruthy();
    session = response.headers()["mcp-session-id"] ?? session;
    const text = await response.text();
    const data = text.startsWith("{")
      ? text
      : text
          .split("\n")
          .filter((line) => line.startsWith("data:"))
          .map((line) => line.slice(5).trim())
          .find((line) => line.startsWith("{"));
    return JSON.parse(data!);
  };
  try {
    await rpc("initialize", {
      protocolVersion: "2025-03-26",
      capabilities: {},
      clientInfo: { name: "instruction-e2e", version: "1" },
    });
    const catalog = await rpc("tools/list", {});
    expect(catalog.result.tools.map((tool: { name: string }) => tool.name)).toContain(
      "create_trade_instruction",
    );
    expect(catalog.result.tools.map((tool: { name: string }) => tool.name)).not.toContain(
      "complete_trade_instruction",
    );
    await page.goto(`${BASE_URL}/goals/${goal.id}`);
    const result = await rpc("tools/call", {
      name: "create_trade_instruction",
      arguments: {
        input: {
          goalId: goal.id,
          notes: "MCP optional note",
          requestId: randomUUID(),
          accountId: account.id,
          asset: {
            symbol: "MCPTI",
            name: "MCP instruction asset",
            quoteMode: "MANUAL",
            quoteCcy: "USD",
            instrumentType: "EQUITY",
          },
          side: "SELL",
          quantity: "1",
        },
      },
    });
    expect(result.result.isError).not.toBe(true);
    await expect(page.getByRole("button").filter({ hasText: "MCPTI" })).toBeVisible({
      timeout: 25_000,
    });
  } finally {
    await page.request.delete(`${api}/agent-access/tokens/${tokenId}`);
  }
});
