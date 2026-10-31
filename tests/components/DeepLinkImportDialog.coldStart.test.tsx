import { render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import { emitTauriEvent } from "../msw/tauriMocks";

const frontendReadyMock = vi.fn();
const toastErrorMock = vi.fn();

vi.mock("@/lib/api/deeplink", () => ({
  deeplinkApi: {
    frontendReady: () => frontendReadyMock(),
    mergeDeeplinkConfig: vi.fn(),
    importFromDeeplink: vi.fn(),
  },
}));

vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => toastErrorMock(...args),
    success: vi.fn(),
    warning: vi.fn(),
  },
}));

const renderDialog = () =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <DeepLinkImportDialog />
    </QueryClientProvider>,
  );

describe("DeepLinkImportDialog pending deep links", () => {
  beforeEach(() => {
    frontendReadyMock.mockReset();
    toastErrorMock.mockReset();
  });

  it("shows a deep link buffered before the frontend was listening", async () => {
    frontendReadyMock.mockResolvedValue({
      kind: "import",
      request: {
        version: "v1",
        resource: "provider",
        app: "claude",
        name: "Pending Provider",
        endpoint: "https://example.com",
      },
    });

    renderDialog();

    expect(await screen.findByText("Pending Provider")).toBeInTheDocument();
    expect(frontendReadyMock).toHaveBeenCalledTimes(1);
  });

  it("reports a buffered parse error", async () => {
    frontendReadyMock.mockResolvedValue({
      kind: "error",
      url: "ccswitch://v1/import?[keys:apiKey]",
      error: "bad link",
    });

    renderDialog();

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith("Failed to parse deep link", {
        description: "bad link",
      }),
    );
  });

  it("still handles live events after signalling readiness", async () => {
    frontendReadyMock.mockResolvedValue(null);

    renderDialog();
    await waitFor(() => expect(frontendReadyMock).toHaveBeenCalled());

    emitTauriEvent("deeplink-import", {
      version: "v1",
      resource: "provider",
      app: "claude",
      name: "Live Provider",
      endpoint: "https://example.com",
    });

    expect(await screen.findByText("Live Provider")).toBeInTheDocument();
  });
});
