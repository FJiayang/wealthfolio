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
    const dialog = page.getByRole("dialog", { name: /^(New instruction|Instruction details)/ });
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
    await expect(page.getByRole("dialog").getByRole("grid")).toBeVisible();
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
      .getByTestId("goal-trade-instructions")
      .getByRole("row")
      .filter({ hasText: symbol });
    await expect(item.getByText("Overdue", { exact: true })).toBeVisible();
    const frozen = item.locator('[data-column-id="actions"]');
    await expect(frozen).toHaveCSS("position", "sticky");
    await expect(frozen.getByRole("button")).toHaveCount(3);
    await frozen.getByRole("button", { name: "Start", exact: true }).click();
    await expect(frozen.getByRole("button", { name: "Start", exact: true })).toBeDisabled();
    await item.getByRole("button", { name: symbol, exact: true }).click();
    await expect(dialog.getByRole("heading", { name: "Transaction" })).toBeVisible();
    await expect(dialog.getByRole("heading", { name: "Financial Details" })).toBeVisible();
    await expect(dialog.getByRole("heading", { name: "Record Info" })).toBeVisible();
    const initialDark = await page
      .locator("html")
      .evaluate((root) => root.classList.contains("dark"));
    for (const dark of [false, true]) {
      await page
        .locator("html")
        .evaluate((root, enabled) => root.classList.toggle("dark", enabled), dark);
      await page.mouse.move(0, 0);
      for (const [name, tokenClasses] of [
        ["Mark complete", "bg-success"],
        ["Cancel instruction", "bg-destructive dark:bg-destructive/60"],
      ]) {
        const button = dialog.getByRole("button", { name, exact: true });
        const color = await button.evaluate((element, classes) => {
          const probe = document.createElement("span");
          probe.className = classes;
          element.appendChild(probe);
          const result = getComputedStyle(probe).backgroundColor;
          probe.remove();
          return result;
        }, tokenClasses);
        await expect(button).toHaveCSS("background-color", color);
      }
      await page.screenshot({
        path: test.info().outputPath(`instruction-detail-${dark ? "dark" : "light"}.png`),
        animations: "disabled",
      });
    }
    await page
      .locator("html")
      .evaluate((root, enabled) => root.classList.toggle("dark", enabled), initialDark);
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
    await dialog.getByRole("button", { name: "Close", exact: true }).click();
    await page.getByRole("button", { name: "Refresh", exact: true }).click();
    await page
      .getByTestId("goal-trade-instructions")
      .getByRole("row")
      .filter({ hasText: symbol })
      .getByRole("button", { name: symbol, exact: true })
      .click();
    await dialog.getByRole("button", { name: "Cancel instruction", exact: true }).click();
    await dialog.getByLabel("Resolution note").fill("No longer needed");
    await dialog.getByRole("button", { name: "Confirm", exact: true }).click();
    await expect(dialog.getByText("Cancelled", { exact: true })).toBeVisible();

    await dialog.getByRole("button", { name: "Close", exact: true }).click();
    const list = page.getByTestId("goal-trade-instructions");
    for (const [label, title, status] of [
      ["Complete", "Mark complete", "COMPLETED"],
      ["Cancel", "Cancel instruction", "CANCELLED"],
    ]) {
      const created = await page.request.post(`${api}/trade-instructions`, {
        data: { ...retryRequest, requestId: randomUUID() },
      });
      expect(created.ok()).toBeTruthy();
      const quickRow = await created.json();
      await list.getByRole("button", { name: "Refresh", exact: true }).click();
      await list
        .getByRole("row")
        .filter({ hasText: symbol })
        .locator('[data-column-id="actions"]')
        .getByRole("button", { name: label, exact: true })
        .click();
      const quickDialog = page.getByRole("dialog", { name: title, exact: true });
      await expect(quickDialog.getByLabel("Resolution note")).toHaveAttribute("required", "");
      await quickDialog.getByLabel("Resolution note").fill("Quick action handled externally");
      await quickDialog.getByRole("button", { name: "Confirm", exact: true }).click();
      await expect(quickDialog).toBeHidden();
      const saved = await (
        await page.request.get(`${api}/trade-instructions/${quickRow.id}`)
      ).json();
      expect(saved.status).toBe(status);
      expect(saved.resolutionNote).toBe("Quick action handled externally");
    }
    // Populate more than one page in reverse deadline order, using the real API.
    for (let index = 24; index >= 0; index--) {
      const response = await page.request.post(`${api}/trade-instructions`, {
        data: {
          ...retryRequest,
          requestId: randomUUID(),
          side: index % 2 ? "SELL" : "BUY",
          expectedCompletionAt: `2027-01-${String(index + 1).padStart(2, "0")}T00:00:00Z`,
          notes: `Pagination ${index}`,
        },
      });
      expect(response.ok()).toBeTruthy();
    }
    await list.getByRole("button", { name: "Refresh", exact: true }).click();
    await expect(list.getByRole("grid")).toHaveAttribute("aria-rowcount", "20");
    await expect(list.getByRole("row").filter({ hasText: "Pagination 0" })).toBeVisible();
    await list.getByRole("button", { name: "Next page", exact: true }).click();
    await expect(list.getByRole("grid")).toHaveAttribute("aria-rowcount", "5");
    await expect(list.getByRole("row").filter({ hasText: "Pagination 20" })).toBeVisible();
    await list
      .getByTestId("instruction-filters")
      .getByRole("button", { name: "Side", exact: true })
      .click();
    await page.getByRole("option", { name: "Sell", exact: true }).click();
    await page.keyboard.press("Escape");
    await expect(list.getByRole("grid")).toHaveAttribute("aria-rowcount", "12");
    await list.getByRole("combobox", { name: "Rows per page" }).click();
    await page.getByRole("option", { name: "50", exact: true }).click();
    await page.reload();
    await expect(list.getByRole("combobox", { name: "Rows per page" })).toHaveText("50");
    await expect(list.getByRole("grid")).toHaveAttribute("aria-rowcount", "25");
    await list.getByRole("button", { name: "Toggle columns", exact: true }).click();
    await expect(page.getByRole("menuitemcheckbox")).toHaveCount(10);
    await expect(page.getByRole("menuitemcheckbox", { name: /actions/i })).toHaveCount(0);
    await page.keyboard.press("Escape");
    const gridViewport = list.getByRole("grid");
    for (const scrollLeft of [0, rtl ? -10000 : 10000]) {
      await gridViewport.evaluate((element, left) => {
        element.scrollLeft = left;
      }, scrollLeft);
      const pinnedCell = gridViewport.locator('[data-column-id="actions"]').first();
      await expect(pinnedCell).toHaveCSS("position", "sticky");
      const viewportBox = await gridViewport.boundingBox();
      const pinnedBox = await pinnedCell.boundingBox();
      expect(pinnedBox!.x).toBeGreaterThanOrEqual(viewportBox!.x - 1);
      expect(pinnedBox!.x + pinnedBox!.width).toBeLessThanOrEqual(
        viewportBox!.x + viewportBox!.width + 1,
      );
    }
    await gridViewport.evaluate((element) => {
      element.scrollLeft = 0;
    });
    await list
      .getByTestId("instruction-filters")
      .getByRole("button", { name: "Expected completion time", exact: true })
      .click();
    const datePopover = page
      .locator('[data-slot="popover-content"]')
      .filter({ has: page.locator("button[data-day]") });
    const days = datePopover.locator("button[data-day]");
    const fromDay = await days.nth(7).getAttribute("data-day");
    const throughDay = await days.nth(8).getAttribute("data-day");
    await days.nth(7).click();
    const dateRequest = page.waitForRequest(
      (request) =>
        request.url().endsWith("/trade-instructions/list") &&
        !!request.postDataJSON()?.expectedCompletionBefore,
    );
    await days.nth(8).click();
    const sent = (await dateRequest).postDataJSON();
    expect(sent.expectedCompletionFrom).toBe(new Date(`${fromDay}T00:00:00+08:00`).toISOString());
    expect(Date.parse(sent.expectedCompletionBefore)).toBe(
      Date.parse(`${throughDay}T00:00:00+08:00`) + 86400000,
    );
    await expect(list.getByRole("grid")).toHaveAttribute("aria-rowcount", "0");
    await datePopover.getByRole("button", { name: "Clear", exact: true }).click();
    await page.keyboard.press("Escape");
    await expect(list.getByRole("grid")).toHaveAttribute("aria-rowcount", "25");
    for (const [direction, firstNote] of [
      ["desc", "Pagination 24"],
      ["asc", "Pagination 0"],
    ]) {
      await list
        .getByRole("grid")
        .getByRole("button", { name: "Expected completion time", exact: true })
        .click();
      const sortedResponse = page.waitForResponse(
        (response) =>
          response.url().endsWith("/trade-instructions/list") &&
          response.request().postDataJSON()?.sort ===
            (direction === "desc" ? "expectedCompletionDesc" : "expectedCompletionAsc"),
      );
      await page.getByRole("menuitemcheckbox", { name: `Sort ${direction}`, exact: true }).click();
      expect((await (await sortedResponse).json()).items[0].notes).toBe(firstNote);
    }
    await list.getByRole("combobox", { name: "Rows per page" }).scrollIntoViewIfNeeded();
    await page.screenshot({
      path: test.info().outputPath(`instruction-grid-${rtl ? "rtl" : "ltr"}.png`),
      fullPage: true,
      animations: "disabled",
    });
    // Leave this preference at the default for the next LTR/RTL run.
    await list.getByRole("combobox", { name: "Rows per page" }).click();
    await page.getByRole("option", { name: "20", exact: true }).click();

    const otherGoal = await (
      await page.request.post(`${api}/goals`, {
        data: { title: "Other goal", goalType: "custom_save_up", targetAmount: 2000 },
      })
    ).json();
    await page.goto(`${BASE_URL}/goals/${otherGoal.id}`);
    await expect(
      page
        .getByTestId("goal-trade-instructions")
        .getByText("No instructions", { exact: true })
        .first(),
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
