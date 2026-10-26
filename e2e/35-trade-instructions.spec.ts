import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";
import { BASE_URL, loginIfNeeded } from "./helpers";

for (const rtl of [false, true]) {
  test(`trade instructions: create, complete and correct (${rtl ? "RTL" : "LTR"})`, async ({
    page,
  }) => {
    test.setTimeout(180_000);
    await loginIfNeeded(page);
    const api = `${BASE_URL}/api/v1`;
    const suffix = Date.now().toString();
    const name = `Instructions ${suffix}`;
    const symbol = `TI${suffix}`;
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
    await page.goto(`${BASE_URL}/trade-instructions${rtl ? "?dir=rtl" : ""}`);
    await page.getByRole("button", { name: "New instruction", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: /^(New instruction|Instruction details)$/ });
    await dialog
      .getByRole("combobox")
      .filter({ hasText: /Select an account/i })
      .click();
    await page.getByRole("option").filter({ hasText: name }).click();
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
    await dialog.getByLabel("Expected completion", { exact: false }).fill("2020-01-01T12:00");
    await dialog.getByLabel("Limit price", { exact: false }).fill("12.34");
    await dialog.getByRole("button", { name: "New instruction", exact: true }).click();
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
      data: { statuses: [], accountId: account.id },
    });
    const row = (await listing.json()).items[0];
    expect(row.status).toBe("COMPLETED");
    expect(row.quantity).toBe("2.123456789");
    expect(row.completedAt).toBeTruthy();
    await dialog.getByRole("button", { name: "Correct completion time", exact: true }).click();
    await dialog.getByLabel("Actual completion", { exact: false }).fill("2020-01-02T12:00");
    await dialog.getByRole("button", { name: "Confirm", exact: true }).click();
    await expect(
      dialog.getByRole("button", { name: "Correct completion time", exact: true }),
    ).toBeVisible();
    const corrected = await (await page.request.get(`${api}/trade-instructions/${row.id}`)).json();
    expect(corrected.completedAt).not.toBe(row.completedAt);
    expect(corrected.closedAt).toBe(row.closedAt);
    expect(corrected.version).toBe(row.version + 1);
    const activities = await (
      await page.request.post(`${api}/activities/search`, {
        data: { page: 0, pageSize: 100, accountIdFilter: [account.id] },
      })
    ).json();
    expect(activities.data).toHaveLength(0);
    const retryRequest = {
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
  const credentials = await page.request.post(`${api}/agent-access/tokens`, {
    data: {
      name: "Instruction E2E",
      scopes: [
        "trade-instructions:read",
        "trade-instructions:create",
        "accounts:read",
        "assets:write",
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
    await page.goto(`${BASE_URL}/trade-instructions`);
    const result = await rpc("tools/call", {
      name: "create_trade_instruction",
      arguments: {
        input: {
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
