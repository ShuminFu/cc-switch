import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi } from "vitest";
import { SyncEncryptionSettings } from "@/components/settings/SyncEncryptionSettings";
import { mockInvoke } from "../msw/invokeRecorder";

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

// TEST-12: IPC-level mocks; the real syncEncryptionApi wrapper runs.

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
  it("enables encryption only with a long enough, confirmed passphrase", async () => {
    const api = mockInvoke({
      sync_encryption_get_status: off,
      sync_encryption_set_passphrase: on,
    });
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
      expect(api.callsTo("sync_encryption_set_passphrase")).toEqual([
        {
          command: "sync_encryption_set_passphrase",
          args: { passphrase: "long passphrase" },
        },
      ]),
    );
    expect(
      await screen.findByText("Passphrase stored in the system keychain."),
    ).toBeTruthy();
  });

  it("turning the switch off disables encryption", async () => {
    const api = mockInvoke({
      sync_encryption_get_status: on,
      sync_encryption_disable: off,
    });
    renderIt();

    await screen.findByText("Passphrase stored in the system keychain.");
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() =>
      expect(api.callsTo("sync_encryption_disable")).toHaveLength(1),
    );
  });
});
