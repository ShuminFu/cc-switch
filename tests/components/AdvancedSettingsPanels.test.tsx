import {
  render as rtlRender,
  screen,
  fireEvent,
  waitFor,
} from "@testing-library/react";
import type { ReactElement } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ConnectivityCheckConfigPanel } from "@/components/usage/ConnectivityCheckConfigPanel";
import { createTestQueryClient } from "../utils/testQueryClient";
import { LogConfigPanel } from "@/components/settings/LogConfigPanel";
import { RectifierConfigPanel } from "@/components/settings/RectifierConfigPanel";
import { TerminalSettings } from "@/components/settings/TerminalSettings";
import { mockInvoke } from "../msw/invokeRecorder";

const render = (ui: ReactElement) =>
  rtlRender(
    <QueryClientProvider client={createTestQueryClient()}>
      {ui}
    </QueryClientProvider>,
  );

const toastErrorMock = vi.hoisted(() => vi.fn());
vi.mock("sonner", () => ({
  toast: { error: toastErrorMock, success: vi.fn() },
}));

const platform = vi.hoisted(() => ({
  isMac: vi.fn(() => false),
  isWindows: vi.fn(() => false),
  isLinux: vi.fn(() => true),
}));
vi.mock("@/lib/platform", () => platform);

describe("LogConfigPanel", () => {
  beforeEach(() => {
    toastErrorMock.mockReset();
  });

  it("loads the saved config and disables the level picker when logging is off", async () => {
    mockInvoke({ get_log_config: { enabled: false, level: "debug" } });
    render(<LogConfigPanel />);

    const toggle = await screen.findByRole("switch");
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("combobox")).toBeDisabled();
  });

  it("saves the merged config when logging is toggled", async () => {
    const api = mockInvoke({
      get_log_config: { enabled: true, level: "warn" },
      set_log_config: true,
    });
    render(<LogConfigPanel />);

    const toggle = await screen.findByRole("switch");
    fireEvent.click(toggle);

    await waitFor(() => expect(api.callsTo("set_log_config")).toHaveLength(1));
    expect(api.callsTo("set_log_config")[0].args).toEqual({
      config: { enabled: false, level: "warn" },
    });
    expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "false");
  });

  it("reverts the toggle and shows the backend error when saving fails", async () => {
    mockInvoke({
      get_log_config: { enabled: true, level: "info" },
      set_log_config: () => {
        throw new Error("disk full");
      },
    });
    render(<LogConfigPanel />);

    fireEvent.click(await screen.findByRole("switch"));

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    expect(String(toastErrorMock.mock.calls[0][0])).toContain("disk full");
    await waitFor(() =>
      expect(screen.getByRole("switch")).toHaveAttribute(
        "aria-checked",
        "true",
      ),
    );
  });
});

describe("RectifierConfigPanel", () => {
  const rectifier = {
    enabled: true,
    requestThinkingSignature: true,
    requestThinkingBudget: false,
    requestMediaFallback: true,
    requestMediaHeuristic: true,
  };
  const optimizer = {
    enabled: false,
    thinkingOptimizer: true,
    cacheInjection: true,
  };

  it("renders loaded values and saves a sub-option change with the full config", async () => {
    const api = mockInvoke({
      get_rectifier_config: rectifier,
      get_optimizer_config: optimizer,
      set_rectifier_config: true,
    });
    render(<RectifierConfigPanel />);

    const [master, signature, budget] = await screen.findAllByRole("switch");
    expect(master).toHaveAttribute("aria-checked", "true");
    expect(signature).toHaveAttribute("aria-checked", "true");
    expect(budget).toHaveAttribute("aria-checked", "false");

    fireEvent.click(budget);
    await waitFor(() =>
      expect(api.callsTo("set_rectifier_config")).toHaveLength(1),
    );
    expect(api.callsTo("set_rectifier_config")[0].args).toEqual({
      config: { ...rectifier, requestThinkingBudget: true },
    });
  });

  it("disables every request sub-option while the rectifier is off", async () => {
    mockInvoke({
      get_rectifier_config: { ...rectifier, enabled: false },
      get_optimizer_config: optimizer,
    });
    render(<RectifierConfigPanel />);

    const [master, ...rest] = await screen.findAllByRole("switch");
    expect(master).not.toBeDisabled();
    // The four request toggles follow the master switch.
    for (const sub of rest.slice(0, 4)) {
      expect(sub).toBeDisabled();
    }
  });
});

describe("config panels when loading fails (FE-UI-10)", () => {
  it("shows an error and no switches when the rectifier config cannot load", async () => {
    const api = mockInvoke({
      get_rectifier_config: () => {
        throw new Error("db locked");
      },
      get_optimizer_config: {
        enabled: false,
        thinkingOptimizer: true,
        cacheInjection: true,
      },
      set_rectifier_config: true,
    });
    render(<RectifierConfigPanel />);

    expect(await screen.findByTestId("config-load-error")).toBeInTheDocument();
    expect(screen.queryAllByRole("switch")).toHaveLength(0);
    expect(api.callsTo("set_rectifier_config")).toHaveLength(0);
  });

  it("keeps the optimizer switches disabled when only the optimizer fails", async () => {
    mockInvoke({
      get_rectifier_config: {
        enabled: true,
        requestThinkingSignature: true,
        requestThinkingBudget: true,
        requestMediaFallback: true,
        requestMediaHeuristic: true,
      },
      get_optimizer_config: () => {
        throw new Error("boom");
      },
    });
    render(<RectifierConfigPanel />);

    expect(await screen.findByTestId("config-load-error")).toBeInTheDocument();
    const switches = screen.getAllByRole("switch");
    for (const optimizerSwitch of switches.slice(5)) {
      expect(optimizerSwitch).toBeDisabled();
    }
  });

  it("shows an error instead of default log settings", async () => {
    mockInvoke({
      get_log_config: () => {
        throw new Error("nope");
      },
    });
    render(<LogConfigPanel />);
    expect(await screen.findByTestId("config-load-error")).toBeInTheDocument();
    expect(screen.queryByRole("switch")).toBeNull();
  });

  it("rejects a blank connectivity timeout instead of saving a default", async () => {
    const api = mockInvoke({
      get_stream_check_config: {
        timeoutSecs: 10,
        maxRetries: 2,
        degradedThresholdMs: 5000,
      },
      save_stream_check_config: null,
    });
    render(<ConnectivityCheckConfigPanel />);

    const timeout = await screen.findByLabelText("Timeout (seconds)");
    expect(timeout).toHaveValue(10);
    fireEvent.change(timeout, { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: /Save/ }));

    expect(
      await screen.findByText(/Enter a whole number between/),
    ).toBeInTheDocument();
    expect(api.callsTo("save_stream_check_config")).toHaveLength(0);

    fireEvent.change(timeout, { target: { value: "12" } });
    fireEvent.click(screen.getByRole("button", { name: /Save/ }));
    await waitFor(() =>
      expect(api.callsTo("save_stream_check_config")).toHaveLength(1),
    );
    expect(api.callsTo("save_stream_check_config")[0].args).toEqual({
      config: { timeoutSecs: 12, maxRetries: 2, degradedThresholdMs: 5000 },
    });
  });
});

describe("TerminalSettings", () => {
  beforeEach(() => {
    platform.isMac.mockReturnValue(false);
    platform.isWindows.mockReturnValue(false);
    platform.isLinux.mockReturnValue(true);
  });

  it("falls back to the platform default terminal when no value is saved", () => {
    render(<TerminalSettings onChange={vi.fn()} />);
    expect(screen.getByRole("combobox")).toHaveTextContent("GNOME Terminal");
  });

  it("shows the saved terminal for the current platform", () => {
    platform.isLinux.mockReturnValue(false);
    platform.isWindows.mockReturnValue(true);
    render(<TerminalSettings value="wt" onChange={vi.fn()} />);
    expect(screen.getByRole("combobox")).toHaveTextContent("Windows Terminal");
  });
});
