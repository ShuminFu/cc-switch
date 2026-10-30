import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SecretsVaultSettings } from "@/components/settings/SecretsVaultSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: { defaultValue?: string }) =>
      opts?.defaultValue ?? key,
  }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));

const api = vi.hoisted(() => ({
  getStatus: vi.fn(),
  setEnabled: vi.fn(),
}));
vi.mock("@/lib/api/secretsVault", () => ({ secretsVaultApi: api }));

const base = {
  enabled: false,
  backend: "keychain",
  available: true,
  unavailableReason: null,
  vaultedSecrets: 0,
  inlineSecrets: 3,
  unresolvedSecrets: 0,
  unresolvedProviders: [],
  warning: null,
};

function renderIt() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <SecretsVaultSettings />
    </QueryClientProvider>,
  );
}

describe("SecretsVaultSettings", () => {
  beforeEach(() => {
    api.getStatus.mockReset();
    api.setEnabled.mockReset();
  });

  it("is off by default and turns the vault on", async () => {
    api.getStatus.mockResolvedValue(base);
    api.setEnabled.mockResolvedValue({
      ...base,
      enabled: true,
      inlineSecrets: 0,
      vaultedSecrets: 3,
    });
    renderIt();

    const toggle = await screen.findByRole("switch");
    expect(toggle).toHaveAttribute("aria-checked", "false");
    fireEvent.click(toggle);

    await waitFor(() => expect(api.setEnabled).toHaveBeenCalledWith(true));
    await waitFor(() =>
      expect(screen.getByRole("switch")).toHaveAttribute(
        "aria-checked",
        "true",
      ),
    );
    expect(screen.getByText(/re-enter the API keys/)).toBeInTheDocument();
  });

  it("refuses to enable without a keychain and says why", async () => {
    api.getStatus.mockResolvedValue({
      ...base,
      backend: "keychain",
      available: false,
      unavailableReason: "keychain unavailable: no D-Bus session",
    });
    renderIt();

    const toggle = await screen.findByRole("switch");
    await waitFor(() => expect(toggle).toBeDisabled());
    expect(screen.getByRole("alert")).toHaveTextContent(/GNOME Keyring/);
  });

  it("lists providers whose keys are missing on this device", async () => {
    api.getStatus.mockResolvedValue({
      ...base,
      enabled: true,
      inlineSecrets: 0,
      vaultedSecrets: 2,
      unresolvedSecrets: 1,
      unresolvedProviders: ["claude/work"],
    });
    renderIt();

    expect(await screen.findByText(/claude\/work/)).toBeInTheDocument();
  });
});
