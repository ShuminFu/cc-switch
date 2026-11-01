import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NavigationProvider, useNavigation } from "@/app/navigation";
import {
  DeepLinkActionHandler,
  resolveProvider,
  type DeepLinkAction,
} from "@/components/deeplink-actions/DeepLinkActionHandler";
import { server } from "../msw/server";
import { getCurrentProviderId, resetProviderState } from "../msw/state";
import { emitTauriEvent } from "../msw/tauriMocks";
import type { Provider } from "@/types";

const toastError = vi.fn();
vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: (...args: unknown[]) => toastError(...args),
    warning: vi.fn(),
    info: vi.fn(),
  },
}));

function Probe() {
  const { view, params, activeApp } = useNavigation();
  return (
    <div data-testid="probe">
      {view}|{params?.tab ?? ""}|{activeApp}
    </div>
  );
}

const queue: DeepLinkAction[] = [];
const useActionQueue = () =>
  server.use(
    http.post("http://tauri.local/take_deeplink_action", () =>
      HttpResponse.json(queue.shift() ?? null),
    ),
  );

const renderHandler = () =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <NavigationProvider>
        <Probe />
        <DeepLinkActionHandler />
      </NavigationProvider>
    </QueryClientProvider>,
  );

describe("DeepLinkActionHandler (PRODUCT-20)", () => {
  beforeEach(() => {
    localStorage.clear();
    resetProviderState();
    queue.length = 0;
    toastError.mockReset();
    useActionQueue();
  });

  it("resolves providers by id, then by unique name", () => {
    const p = (id: string, name: string) => ({ id, name }) as Provider;
    const providers = { a: p("a", "Relay"), b: p("b", "Other") };
    expect(resolveProvider(providers, "a")?.id).toBe("a");
    expect(resolveProvider(providers, "relay")?.id).toBe("a");
    expect(resolveProvider(providers, "missing")).toBeUndefined();
    const dup = { a: p("a", "Same"), b: p("b", "same") };
    expect(resolveProvider(dup, "same")).toBeUndefined();
  });

  it("opens the view buffered before the frontend was ready", async () => {
    queue.push({ kind: "open", view: "usage", app: "codex" });
    renderHandler();
    await waitFor(() =>
      expect(screen.getByTestId("probe").textContent).toBe(
        "settings|usage|codex",
      ),
    );
  });

  it("asks before switching and switches on confirm", async () => {
    renderHandler();
    queue.push({ kind: "switch", app: "claude", provider: "Claude Custom" });
    emitTauriEvent("deeplink-action", null);

    expect(
      await screen.findByText(/switch Claude to “Claude Custom”/),
    ).toBeInTheDocument();
    expect(getCurrentProviderId("claude")).toBe("claude-1");
    fireEvent.click(screen.getByRole("button", { name: "Switch" }));
    await waitFor(() =>
      expect(getCurrentProviderId("claude")).toBe("claude-2"),
    );
  });

  it("reports an unknown provider instead of switching", async () => {
    renderHandler();
    queue.push({ kind: "switch", app: "claude", provider: "nope" });
    emitTauriEvent("deeplink-action", null);
    await waitFor(() =>
      expect(toastError).toHaveBeenCalledWith(expect.stringContaining("nope")),
    );
    expect(getCurrentProviderId("claude")).toBe("claude-1");
  });
});
