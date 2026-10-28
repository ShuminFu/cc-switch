import { render, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import { emitTauriEvent } from "../msw/tauriMocks";

const toastErrorMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: (...args: unknown[]) => toastErrorMock(...args),
    warning: vi.fn(),
  },
}));

describe("DeepLinkImportDialog", () => {
  it("never logs the deeplink URL (which may carry secrets) on deeplink-error", async () => {
    const consoleErrorSpy = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    const queryClient = new QueryClient();

    render(
      <QueryClientProvider client={queryClient}>
        <DeepLinkImportDialog />
      </QueryClientProvider>,
    );

    // Wait for listeners to register (listen() is async)
    await Promise.resolve();
    await Promise.resolve();

    emitTauriEvent("deeplink-error", {
      url: "ccswitch://v1/import?resource=bogus&apiKey=sk-secret",
      error: "Unsupported resource type: bogus",
    });

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());

    expect(consoleErrorSpy).toHaveBeenCalled();
    const logged = JSON.stringify(consoleErrorSpy.mock.calls);
    expect(logged).not.toContain("sk-secret");
    expect(logged).toContain("Unsupported resource type: bogus");

    consoleErrorSpy.mockRestore();
  });
});
