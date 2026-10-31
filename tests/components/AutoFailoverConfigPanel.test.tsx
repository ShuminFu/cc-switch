import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { AutoFailoverConfigPanel } from "@/components/proxy/AutoFailoverConfigPanel";
import { createTestQueryClient } from "../utils/testQueryClient";
import { mockInvoke } from "../msw/invokeRecorder";

const toastMock = vi.hoisted(() => ({ error: vi.fn(), success: vi.fn() }));
vi.mock("sonner", () => ({ toast: toastMock }));

// `t(key, "default")` and `t(key, { defaultValue })` both return the key.
const config = {
  appType: "claude",
  enabled: true,
  autoFailoverEnabled: true,
  maxRetries: 2,
  streamingFirstByteTimeout: 30,
  streamingIdleTimeout: 90,
  nonStreamingTimeout: 300,
  circuitFailureThreshold: 4,
  circuitSuccessThreshold: 2,
  circuitTimeoutSeconds: 45,
  circuitErrorRateThreshold: 0.25,
  circuitMinRequests: 8,
};

const renderPanel = () =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <AutoFailoverConfigPanel appType="claude" />
    </QueryClientProvider>,
  );

const maxRetries = () =>
  screen.getByLabelText("Max Retries") as HTMLInputElement;

beforeEach(() => {
  toastMock.error.mockReset();
  toastMock.success.mockReset();
});

describe("AutoFailoverConfigPanel", () => {
  it("loads the per-app config into the form (error rate shown as a percentage)", async () => {
    const api = mockInvoke({ get_proxy_config_for_app: config });
    renderPanel();

    await waitFor(() => expect(maxRetries()).toHaveValue(2));
    expect(api.callsTo("get_proxy_config_for_app")[0].args).toEqual({
      appType: "claude",
    });
    expect(screen.getByDisplayValue("25")).toBeInTheDocument();
    expect(screen.getByDisplayValue("45")).toBeInTheDocument();
  });

  it("refuses out-of-range values without saving", async () => {
    const api = mockInvoke({
      get_proxy_config_for_app: config,
      update_proxy_config_for_app: null,
    });
    renderPanel();

    await waitFor(() => expect(maxRetries()).toHaveValue(2));
    fireEvent.change(maxRetries(), { target: { value: "11" } });
    fireEvent.click(screen.getByRole("button", { name: /Save/ }));

    await waitFor(() => expect(toastMock.error).toHaveBeenCalled());
    expect(String(toastMock.error.mock.calls[0][0])).toContain(
      "The following fields are out of valid range",
    );
    expect(api.callsTo("update_proxy_config_for_app")).toHaveLength(0);
    // The offending field is flagged inline (FE-UI-20).
    expect(maxRetries()).toHaveAttribute("aria-invalid", "true");
    expect(screen.getAllByText(/Enter a whole number from/)).toHaveLength(1);
  });

  it("saves the full config, converting the error rate back to a ratio", async () => {
    const api = mockInvoke({
      get_proxy_config_for_app: config,
      update_proxy_config_for_app: null,
    });
    renderPanel();

    await waitFor(() => expect(maxRetries()).toHaveValue(2));
    fireEvent.change(maxRetries(), { target: { value: "5" } });
    fireEvent.click(screen.getByRole("button", { name: /Save/ }));

    await waitFor(() =>
      expect(api.callsTo("update_proxy_config_for_app")).toHaveLength(1),
    );
    expect(api.callsTo("update_proxy_config_for_app")[0].args).toEqual({
      config: { ...config, maxRetries: 5, circuitErrorRateThreshold: 0.25 },
    });
  });

  it("reset restores the loaded values", async () => {
    mockInvoke({ get_proxy_config_for_app: config });
    renderPanel();

    await waitFor(() => expect(maxRetries()).toHaveValue(2));
    fireEvent.change(maxRetries(), { target: { value: "7" } });
    expect(maxRetries()).toHaveValue(7);
    fireEvent.click(screen.getByRole("button", { name: /Reset/ }));
    expect(maxRetries()).toHaveValue(2);
  });
});
