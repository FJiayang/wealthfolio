import { act, fireEvent, render, screen, waitFor } from "@/test/render";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MarketDataProviderSetting } from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";
import { FutuCredentialsForm } from "./futu-credentials-form";

const adapters = vi.hoisted(() => ({
  getSecret: vi.fn(),
  setSecret: vi.fn(),
  deleteSecret: vi.fn(),
  updateMarketDataProviderSettings: vi.fn(),
}));
vi.mock("@/adapters", () => adapters);
const credentials = { appKeyId: "synthetic-app", privateKey: "synthetic-private-key" };
const provider = {
  id: "FUTU",
  enabled: true,
  priority: 8,
  hasApiKey: true,
} as MarketDataProviderSetting;
function setup() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <FutuCredentialsForm provider={provider} />
    </QueryClientProvider>,
  );
  return client;
}
async function reveal() {
  fireEvent.click(screen.getByRole("button", { name: /Show \/ edit credentials/i }));
  await screen.findByLabelText("Private key (PEM)");
}

describe("Futu credentials", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    adapters.getSecret.mockResolvedValue(JSON.stringify(credentials));
    adapters.setSecret.mockResolvedValue(undefined);
    adapters.deleteSecret.mockResolvedValue(undefined);
    adapters.updateMarketDataProviderSettings.mockResolvedValue(undefined);
  });
  it("reads only on reveal, saves both fields, refreshes and keeps secrets out of caches", async () => {
    const client = setup();
    expect(adapters.getSecret).not.toHaveBeenCalled();
    expect(screen.queryByLabelText("Private key (PEM)")).not.toBeInTheDocument();
    await reveal();
    expect(adapters.getSecret).toHaveBeenCalledWith("FUTU");
    fireEvent.change(screen.getByLabelText("AppKeyId"), { target: { value: "replacement" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Credentials saved.");
    expect(adapters.setSecret).toHaveBeenCalledWith(
      "FUTU",
      JSON.stringify({ ...credentials, appKeyId: "replacement" }),
    );
    expect(adapters.updateMarketDataProviderSettings).not.toHaveBeenCalled();
    expect(client.getMutationCache().getAll()).toHaveLength(0);
    expect(client.getQueryCache().getAll()).toHaveLength(0);
    expect(screen.queryByLabelText("Private key (PEM)")).not.toBeInTheDocument();
  });
  it("preserves settings changed while the secret write is pending", async () => {
    let finishWrite!: () => void;
    adapters.setSecret.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          finishWrite = resolve;
        }),
    );
    const client = setup();
    await reveal();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(adapters.setSecret).toHaveBeenCalledOnce());
    client.setQueryData(
      [QueryKeys.MARKET_DATA_PROVIDER_SETTINGS],
      [{ ...provider, enabled: false, priority: 3 }],
    );
    await act(async () => {
      finishWrite();
      await Promise.resolve();
    });
    await screen.findByText("Credentials saved.");
    expect(adapters.updateMarketDataProviderSettings).not.toHaveBeenCalled();
  });
  it("deletes without reading the private key and refreshes the client", async () => {
    setup();
    fireEvent.click(screen.getByRole("button", { name: "Delete credentials" }));
    await screen.findByText("Credentials deleted.");
    expect(adapters.deleteSecret).toHaveBeenCalledWith("FUTU");
    expect(adapters.getSecret).not.toHaveBeenCalled();
    expect(adapters.updateMarketDataProviderSettings).not.toHaveBeenCalled();
  });
  it("reports a refresh failure without exposing server error text or success", async () => {
    adapters.setSecret.mockRejectedValue(
      new Error("Credential changed, but market data client refresh failed."),
    );
    setup();
    await reveal();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText(/Credentials were stored, but validation or client refresh failed/);
    expect(screen.queryByText("Credentials saved.")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).not.toHaveTextContent(credentials.privateKey);
  });
  it("does not refresh after a failed secret write", async () => {
    adapters.setSecret.mockRejectedValue(new Error("unavailable"));
    setup();
    await reveal();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Unable to save credentials.");
    expect(adapters.updateMarketDataProviderSettings).not.toHaveBeenCalled();
  });
  it("keeps edited values when hiding and showing", async () => {
    setup();
    await reveal();
    fireEvent.change(screen.getByLabelText("AppKeyId"), { target: { value: "edited" } });
    fireEvent.click(screen.getByRole("button", { name: "Hide credentials" }));
    await reveal();
    await waitFor(() => expect(screen.getByLabelText("AppKeyId")).toHaveValue("edited"));
    expect(adapters.getSecret).toHaveBeenCalledOnce();
  });
});
