import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ClaudeDesktopRouteToggle } from "@/components/proxy/ClaudeDesktopRouteToggle";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: toastMock }));

const runningOverview = {
  status: {
    running: true,
    address: "127.0.0.1",
    port: 15721,
    active_targets: [],
  },
  takeover: { claude: false, codex: false, gemini: false, grokbuild: false },
};

const renderToggle = () =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ClaudeDesktopRouteToggle />
    </QueryClientProvider>,
  );

describe("ClaudeDesktopRouteToggle (PROXY-30)", () => {
  beforeEach(() => {
    toastMock.warning.mockReset();
  });

  it("explains why stopping is refused while Claude Desktop routes through the proxy", async () => {
    const api = mockInvoke({
      get_proxy_overview: runningOverview,
      get_proxy_status: runningOverview.status,
      get_claude_desktop_proxy_dependency: true,
      stop_proxy_server: null,
    });
    const { container } = renderToggle();

    const toggle = await screen.findByRole("switch");
    await waitFor(() =>
      expect(toggle).toHaveAttribute("data-state", "checked"),
    );
    await waitFor(() =>
      expect(container.firstElementChild?.getAttribute("title")).toContain(
        "Switch Claude Desktop to a direct provider first",
      ),
    );

    fireEvent.click(toggle);
    await waitFor(() =>
      expect(toastMock.warning).toHaveBeenCalledWith(
        expect.stringContaining("Claude Desktop still sends its requests"),
        { duration: 5000 },
      ),
    );
    expect(api.callsTo("stop_proxy_server")).toHaveLength(0);
  });

  it("stops normally once Claude Desktop no longer depends on the route", async () => {
    const api = mockInvoke({
      get_proxy_overview: runningOverview,
      get_proxy_status: runningOverview.status,
      get_claude_desktop_proxy_dependency: false,
      stop_proxy_server: null,
    });
    renderToggle();

    const toggle = await screen.findByRole("switch");
    await waitFor(() =>
      expect(toggle).toHaveAttribute("data-state", "checked"),
    );
    fireEvent.click(toggle);
    await waitFor(() =>
      expect(api.callsTo("stop_proxy_server")).toHaveLength(1),
    );
    expect(toastMock.warning).not.toHaveBeenCalled();
  });
});
