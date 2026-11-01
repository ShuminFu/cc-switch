import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { ProviderHealthPanel } from "@/components/providers/ProviderHealthPanel";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

const provider = (id: string, name: string, category?: string): Provider =>
  ({ id, name, settingsConfig: {}, category }) as Provider;

describe("ProviderHealthPanel (FE-PROV-34)", () => {
  it("checks every non-official provider and shows status, latency and failure counters", async () => {
    const api = mockInvoke({
      get_providers_health: [
        {
          provider_id: "relay",
          app_type: "claude",
          is_healthy: false,
          consecutive_failures: 3,
          last_success_at: null,
          last_failure_at: null,
          last_error: "HTTP 502",
          updated_at: new Date(0).toISOString(),
        },
      ],
      stream_check_provider: ({ providerId }: Record<string, unknown>) => ({
        status: providerId === "relay" ? "degraded" : "operational",
        success: true,
        message: "",
        responseTimeMs: providerId === "relay" ? 2400 : 120,
        testedAt: 0,
        retryCount: 0,
      }),
    });
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderHealthPanel
          open
          appId="claude"
          providers={[
            provider("official", "Claude Official", "official"),
            provider("relay", "Relay"),
            provider("backup", "Backup"),
          ]}
          onOpenChange={vi.fn()}
        />
      </QueryClientProvider>,
    );

    const relay = screen.getByTestId("health-row-relay");
    expect(
      await within(relay).findByText("3 failures in a row"),
    ).toBeInTheDocument();
    expect(
      within(screen.getByTestId("health-row-official")).getByText(
        "Official: not probed",
      ),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Check all (2)" }));
    await waitFor(() =>
      expect(api.callsTo("stream_check_provider")).toHaveLength(2),
    );
    expect(
      api.callsTo("stream_check_provider").map((c) => c.args.providerId),
    ).toEqual(["relay", "backup"]);
    expect(await within(relay).findByText(/2400 ms/)).toBeInTheDocument();
    expect(
      within(screen.getByTestId("health-row-backup")).getByText(/120 ms/),
    ).toBeInTheDocument();
  });
});
