import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SyncEncryptionSettings } from "@/components/settings/SyncEncryptionSettings";

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const api = vi.hoisted(() => ({
  getStatus: vi.fn(),
  setPassphrase: vi.fn(),
  disable: vi.fn(),
}));
vi.mock("@/lib/api/syncEncryption", () => ({ syncEncryptionApi: api }));

const off = {
  enabled: false,
  hasPassphrase: false,
  storage: "none",
  minPassphraseLength: 8,
};
const on = { ...off, enabled: true, hasPassphrase: true, storage: "keychain" };

function renderIt() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <SyncEncryptionSettings />
    </QueryClientProvider>,
  );
}

describe("SyncEncryptionSettings", () => {
  beforeEach(() => {
    api.getStatus.mockReset();
    api.setPassphrase.mockReset();
    api.disable.mockReset();
  });

  it("enables encryption only with a long enough, confirmed passphrase", async () => {
    api.getStatus.mockResolvedValue(off);
    api.setPassphrase.mockResolvedValue(on);
    renderIt();

    fireEvent.click(await screen.findByRole("switch"));
    const [pass, confirm] = screen.getAllByPlaceholderText(/passphrase/i);
    const save = screen.getByRole("button", { name: "Save passphrase" });

    fireEvent.change(pass, { target: { value: "short" } });
    expect(save).toBeDisabled();

    fireEvent.change(pass, { target: { value: "long passphrase" } });
    fireEvent.change(confirm, { target: { value: "long passphrasX" } });
    expect(save).toBeDisabled();
    expect(screen.getByText("The passphrases do not match.")).toBeTruthy();

    fireEvent.change(confirm, { target: { value: "long passphrase" } });
    expect(save).not.toBeDisabled();
    fireEvent.click(save);

    await waitFor(() =>
      expect(api.setPassphrase).toHaveBeenCalledWith("long passphrase"),
    );
    expect(
      await screen.findByText("Passphrase stored in the system keychain."),
    ).toBeTruthy();
  });

  it("turning the switch off disables encryption", async () => {
    api.getStatus.mockResolvedValue(on);
    api.disable.mockResolvedValue(off);
    renderIt();

    await screen.findByText("Passphrase stored in the system keychain.");
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() => expect(api.disable).toHaveBeenCalled());
  });
});
