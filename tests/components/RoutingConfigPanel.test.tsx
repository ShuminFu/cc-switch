import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { RoutingConfigPanel } from "@/components/proxy/RoutingConfigPanel";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

describe("RoutingConfigPanel (PROXY-19)", () => {
  it("loads the app's routing config and saves strategy, sticky sessions and concurrency", async () => {
    const api = mockInvoke({
      get_proxy_routing_config: {
        strategy: "priority",
        stickySessions: false,
        stickyTtlSecs: 0,
        maxConcurrentPerProvider: 0,
        weights: {},
      },
      get_failover_queue: [
        { providerId: "p1", providerName: "Relay" },
        { providerId: "p2", providerName: "Backup" },
      ],
      set_proxy_routing_config: null,
    });
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <RoutingConfigPanel appType="codex" />
      </QueryClientProvider>,
    );

    const save = await screen.findByRole("button", { name: "Save" });
    expect(save).toBeDisabled();
    expect(api.callsTo("get_proxy_routing_config")[0]?.args).toEqual({
      appType: "codex",
    });

    fireEvent.click(screen.getByRole("radio", { name: "Weighted" }));
    const weight = await screen.findByLabelText("Relay");
    fireEvent.change(weight, { target: { value: "3" } });
    fireEvent.click(screen.getByRole("switch", { name: "Sticky sessions" }));
    fireEvent.change(
      screen.getByLabelText(/Max in-flight requests per provider/),
      { target: { value: "2" } },
    );

    expect(save).toBeEnabled();
    fireEvent.click(save);

    await waitFor(() =>
      expect(api.callsTo("set_proxy_routing_config")).toHaveLength(1),
    );
    expect(api.callsTo("set_proxy_routing_config")[0].args).toEqual({
      appType: "codex",
      config: {
        strategy: "weighted",
        stickySessions: true,
        stickyTtlSecs: 0,
        maxConcurrentPerProvider: 2,
        weights: { p1: 3 },
      },
    });
    await waitFor(() => expect(save).toBeDisabled());
  });
});
