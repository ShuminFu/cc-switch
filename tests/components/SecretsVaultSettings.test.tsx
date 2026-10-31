import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi } from "vitest";
import { SecretsVaultSettings } from "@/components/settings/SecretsVaultSettings";
import { mockInvoke } from "../msw/invokeRecorder";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));

// TEST-12: IPC-level mocks; the real secretsVaultApi wrapper runs.

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
  it("is off by default and turns the vault on", async () => {
    let status = base;
    const api = mockInvoke({
      get_provider_secret_vault_status: () => status,
      set_provider_secret_vault: () => {
        status = {
          ...base,
          enabled: true,
          inlineSecrets: 0,
          vaultedSecrets: 3,
        };
        return status;
      },
    });
    renderIt();

    const toggle = await screen.findByRole("switch");
    expect(toggle).toHaveAttribute("aria-checked", "false");
    fireEvent.click(toggle);

    await waitFor(() =>
      expect(api.callsTo("set_provider_secret_vault")).toEqual([
        { command: "set_provider_secret_vault", args: { enabled: true } },
      ]),
    );
    await waitFor(() =>
      expect(screen.getByRole("switch")).toHaveAttribute(
        "aria-checked",
        "true",
      ),
    );
    expect(screen.getByText(/re-enter the API keys/)).toBeInTheDocument();
  });

  it("refuses to enable without a keychain and says why", async () => {
    mockInvoke({
      get_provider_secret_vault_status: {
        ...base,
        backend: "keychain",
        available: false,
        unavailableReason: "keychain unavailable: no D-Bus session",
      },
    });
    renderIt();

    const toggle = await screen.findByRole("switch");
    await waitFor(() => expect(toggle).toBeDisabled());
    expect(screen.getByRole("alert")).toHaveTextContent(/GNOME Keyring/);
  });

  it("lists providers whose keys are missing on this device", async () => {
    mockInvoke({
      get_provider_secret_vault_status: {
        ...base,
        enabled: true,
        inlineSecrets: 0,
        vaultedSecrets: 2,
        unresolvedSecrets: 1,
        unresolvedProviders: ["claude/work"],
      },
    });
    renderIt();

    expect(await screen.findByText(/claude\/work/)).toBeInTheDocument();
  });
});
