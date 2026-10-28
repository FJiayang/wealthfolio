import { act, render, screen, waitFor } from "@/test/render";
import userEvent from "@testing-library/user-event";
import React from "react";
import { MemoryRouter, useLocation } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ActionPalette } from "@/components/action-palette";
import {
  clearAddonContributions,
  clearAllContributions,
  ingestAddonContributions,
} from "@/addons/contribution-registry";
import { ActivityImportButton } from "./addon-import-actions";
import { useAddonImportActions } from "../hooks/use-addon-import-actions";

vi.mock("@/adapters", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/adapters")>()),
  logger: { warn: vi.fn(), debug: vi.fn() },
}));
vi.mock("@/hooks", () => ({ useHapticFeedback: () => ({ triggerHaptic: vi.fn() }) }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

function Harness({ mobile = false }: { mobile?: boolean }) {
  const items = useAddonImportActions("investment-import");
  const spending = useAddonImportActions("spending-import");
  const location = useLocation();
  const [open, setOpen] = React.useState(false);
  return (
    <>
      <output>{location.pathname}</output>
      <output data-testid="spending-count">{spending.length}</output>
      {mobile ? (
        <ActivityImportButton items={items} />
      ) : (
        <ActionPalette
          open={open}
          onOpenChange={setOpen}
          groups={items.length ? [{ items }] : []}
        />
      )}
    </>
  );
}

function contribute() {
  ingestAddonContributions("import-addon", {
    id: "import-addon",
    name: "Import addon",
    version: "1.0.0",
    contributes: {
      routes: [{ id: "investment" }, { id: "spending", path: "spending" }],
      links: {
        "investment-import": [
          { route: "investment", label: "Investment statement", icon: "receipt" },
        ],
        "spending-import": [{ route: "spending", label: "Spending statement" }],
      },
    },
  });
}

afterEach(() => clearAllContributions());

describe("addon import actions", () => {
  it.each([false, true])("navigates and closes the picker (mobile=%s)", async (mobile) => {
    contribute();
    const user = userEvent.setup();
    render(
      <MemoryRouter>
        <Harness mobile={mobile} />
      </MemoryRouter>,
    );
    expect(screen.getByTestId("spending-count")).toHaveTextContent("1");
    await user.click(
      screen.getByRole("button", { name: mobile ? "common:import" : "common:layout.more_options" }),
    );
    expect(screen.queryByRole("button", { name: "Spending statement" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Investment statement" }));
    expect(screen.getByText("/addons/import-addon")).toBeInTheDocument();
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "Investment statement" }),
      ).not.toBeInTheDocument(),
    );
  });

  it("retains the direct CSV link without addons and reacts to enable/disable", async () => {
    const user = userEvent.setup();
    render(
      <MemoryRouter>
        <Harness mobile />
      </MemoryRouter>,
    );
    expect(screen.getByRole("link", { name: "common:import" })).toHaveAttribute("href", "/import");
    act(contribute);
    expect(screen.queryByRole("link", { name: "common:import" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "common:import" }));
    await user.click(screen.getByRole("button", { name: "activity:page.import_from_csv" }));
    expect(screen.getByText("/import")).toBeInTheDocument();
    act(() => clearAddonContributions("import-addon"));
    expect(screen.getByRole("link", { name: "common:import" })).toHaveAttribute("href", "/import");
    expect(screen.getByTestId("spending-count")).toHaveTextContent("0");
  });
});
