import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi } from "vitest";
import { FailoverToggle } from "@/components/proxy/FailoverToggle";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const overview = (takeover: Record<string, boolean>) => ({
  status: { running: false, active_targets: [] },
  takeover,
});
const renderToggle = () =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <FailoverToggle activeApp="claude" />
    </QueryClientProvider>,
  );

describe("FailoverToggle (FE-UI-56)", () => {
  it("can turn failover off when takeover is already off", async () => {
    const api = mockInvoke({
      get_auto_failover_enabled: true,
      get_proxy_overview: overview({ claude: false }),
      set_auto_failover_enabled: null,
    });
    renderToggle();

    const toggle = await screen.findByRole("switch");
    await waitFor(() =>
      expect(toggle).toHaveAttribute("data-state", "checked"),
    );
    expect(toggle).not.toBeDisabled();

    fireEvent.click(toggle);
    await waitFor(() =>
      expect(api.callsTo("set_auto_failover_enabled")).toHaveLength(1),
    );
    expect(api.callsTo("set_auto_failover_enabled")[0].args).toEqual({
      appType: "claude",
      enabled: false,
    });
  });

  it("cannot turn failover on while takeover is off", async () => {
    mockInvoke({
      get_auto_failover_enabled: false,
      get_proxy_overview: overview({ claude: false }),
    });
    renderToggle();
    const toggle = await screen.findByRole("switch");
    await waitFor(() => expect(toggle).toBeDisabled());
  });
});
