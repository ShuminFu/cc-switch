import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import type { DeepLinkImportRequestV2 } from "@/components/deeplink/api";
import { queryKeys } from "@/lib/query/keys";
// The backend's signed-bundle fixture (src-tauri/src/deeplink/v2.rs tests):
// the dialog must render every item the backend accepts (TEST-32).
import envelope from "../../src-tauri/src/deeplink/testdata/signed_envelope.json";
import { mockInvoke } from "../msw/invokeRecorder";
import { emitTauriEvent } from "../msw/tauriMocks";

const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
  info: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: toastMock }));

const b64 = (text: string) =>
  btoa(String.fromCharCode(...new TextEncoder().encode(text)));

function renderDialog() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const invalidate = vi.spyOn(client, "invalidateQueries");
  render(
    <QueryClientProvider client={client}>
      <DeepLinkImportDialog />
    </QueryClientProvider>,
  );
  return { invalidate };
}

async function emit(request: DeepLinkImportRequestV2) {
  await act(async () => {
    emitTauriEvent("deeplink-import", request);
  });
}

async function clickImport() {
  const button = await screen.findByRole("button", { name: /^Import$/ });
  await waitFor(() => expect(button).not.toBeDisabled(), { timeout: 3000 });
  fireEvent.click(button);
}

const provider: DeepLinkImportRequestV2 = {
  version: "v1",
  resource: "provider",
  app: "claude",
  name: "Flow Relay",
  endpoint: "https://relay.example.com",
  apiKey: "sk-flow-placeholder",
  nonce: "n-provider",
};

describe("DeepLinkImportDialog flows (TEST-32)", () => {
  it("cancel discards the pending link and never imports", async () => {
    const api = mockInvoke({
      import_from_deeplink_unified: { type: "provider", id: "x" },
      discard_pending_deeplink: true,
    });
    renderDialog();
    await emit(provider);
    expect(await screen.findByText("Flow Relay")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /cancel/i }));

    await waitFor(() =>
      expect(api.callsTo("discard_pending_deeplink")).toEqual([
        { command: "discard_pending_deeplink", args: { nonce: "n-provider" } },
      ]),
    );
    expect(api.callsTo("import_from_deeplink_unified")).toHaveLength(0);
    await waitFor(() =>
      expect(screen.queryByText("Flow Relay")).not.toBeInTheDocument(),
    );
  });

  it("a successful provider import invalidates that app's provider list", async () => {
    const { invalidate } = renderDialog();
    await emit(provider);
    await screen.findByText("Flow Relay");
    await clickImport();

    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.providers.byApp("claude"),
      }),
    );
    expect(toastMock.success).toHaveBeenCalledWith(
      "Import successful",
      expect.objectContaining({
        description: 'Provider "Flow Relay" has been successfully imported',
      }),
    );
    await waitFor(() =>
      expect(screen.queryByText("Flow Relay")).not.toBeInTheDocument(),
    );
  });

  it("a failed import shows the backend error and keeps the dialog open", async () => {
    mockInvoke({
      import_from_deeplink_unified: () => {
        throw new Error("provider already exists");
      },
    });
    renderDialog();
    await emit(provider);
    await screen.findByText("Flow Relay");
    await clickImport();

    await waitFor(() =>
      expect(toastMock.error).toHaveBeenCalledWith(
        "Failed to import",
        expect.objectContaining({
          description: expect.stringContaining("provider already exists"),
        }),
      ),
    );
    expect(screen.getByText("Flow Relay")).toBeInTheDocument();
  });

  it("a prompt import refreshes the prompt list of its app", async () => {
    const { invalidate } = renderDialog();
    await emit({
      version: "v1",
      resource: "prompt",
      app: "codex",
      name: "Review Rules",
      content: b64("# Review carefully"),
      nonce: "n-prompt",
    });
    await screen.findByText("Review Rules");
    await clickImport();

    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.prompts.byApp("codex"),
      }),
    );
    expect(toastMock.success).toHaveBeenCalledWith(
      "Prompt imported successfully",
      expect.anything(),
    );
  });

  it("renders every item of the backend's signed-bundle fixture", async () => {
    const items = envelope.items.map((item) => {
      const { config, ...rest } = item as typeof item & { config?: unknown };
      return {
        ...rest,
        version: "v2",
        ...(config ? { config: b64(JSON.stringify(config)) } : {}),
      } as DeepLinkImportRequestV2;
    });
    renderDialog();
    await emit({
      ...items[0],
      nonce: "n-fixture",
      bundle: {
        version: envelope.v,
        items,
        signature: {
          status: "verified",
          publisher: envelope.publisher,
          keyId: "892C52781F9B9986",
          trust: "new",
        },
      },
    });

    const badge = await screen.findByTestId("deeplink-signed-badge");
    expect(badge).toHaveTextContent(envelope.publisher);
    envelope.items.forEach((_, index) =>
      expect(
        screen.getByTestId(`deeplink-bundle-item-${index}`),
      ).toBeInTheDocument(),
    );
    expect(screen.getByTestId("deeplink-bundle-item-0")).toHaveTextContent(
      "Example Relay",
    );
    expect(screen.getByTestId("deeplink-bundle-item-1")).toHaveTextContent(
      "example-docs",
    );
    expect(screen.getByTestId("deeplink-bundle-item-2")).toHaveTextContent(
      "Example Guidelines",
    );
  });
});
