import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "@/i18n/i18n";
import type { AgentAccessToken } from "@/adapters";
import { PatEditDialog } from "./pat-edit-dialog";

const token: AgentAccessToken = {
  id: "key-1",
  name: "Client",
  tokenPrefix: "testprefix",
  fingerprint: "sha256:test",
  scopes: ["accounts:read"],
  createdAt: "2026-01-01T00:00:00Z",
  expiresAt: null,
  lastUsedAt: null,
  revokedAt: null,
};

describe("PatEditDialog", () => {
  it("preserves the selection on failure, then saves without exposing a credential", async () => {
    const save = vi.fn().mockRejectedValueOnce(new Error("failed")).mockResolvedValueOnce({});
    const close = vi.fn();
    render(
      <I18nextProvider i18n={i18n}>
        <PatEditDialog token={token} onClose={close} onSave={save} isSaving={false} />
      </I18nextProvider>,
    );
    const assets = screen.getByRole("checkbox", { name: /Register and update investment assets/ });
    expect(assets).toHaveAttribute("aria-checked", "false");
    fireEvent.click(assets);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    expect(close).not.toHaveBeenCalled();
    expect(assets).toHaveAttribute("aria-checked", "true");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(close).toHaveBeenCalledOnce());
    expect(save).toHaveBeenLastCalledWith(["assets:write", "accounts:read"]);
    expect(screen.queryByText(/wfp_/)).toBeNull();
  });
  it("locks required scopes and cancels without saving", () => {
    const save = vi.fn();
    const close = vi.fn();
    render(
      <I18nextProvider i18n={i18n}>
        <PatEditDialog token={token} onClose={close} onSave={save} isSaving={false} />
      </I18nextProvider>,
    );
    fireEvent.click(
      screen.getByRole("checkbox", { name: /Commit activities, update existing ones/ }),
    );
    const required = screen.getByRole("checkbox", { name: /Prepare draft activities for review/ });
    expect(required).toBeDisabled();
    expect(required).toHaveAttribute("aria-checked", "true");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(save).not.toHaveBeenCalled();
    expect(close).toHaveBeenCalledOnce();
  });
});
