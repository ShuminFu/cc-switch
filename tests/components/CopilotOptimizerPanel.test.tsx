import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  CopilotOptimizerPanel,
  DEFAULT_COPILOT_OPTIMIZER_CONFIG,
} from "@/components/settings/CopilotOptimizerPanel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const getMock = vi.fn();
const setMock = vi.fn();

vi.mock("@/lib/api/settings", () => ({
  settingsApi: {
    getCopilotOptimizerConfig: (...args: unknown[]) => getMock(...args),
    setCopilotOptimizerConfig: (...args: unknown[]) => setMock(...args),
  },
}));

describe("CopilotOptimizerPanel", () => {
  beforeEach(() => {
    getMock.mockReset();
    setMock.mockReset();
    getMock.mockResolvedValue({
      ...DEFAULT_COPILOT_OPTIMIZER_CONFIG,
      warmupDowngrade: false,
      warmupModel: "gpt-5-mini",
    });
    setMock.mockResolvedValue(true);
  });

  it("loads the stored config and persists a toggled switch", async () => {
    render(<CopilotOptimizerPanel />);

    const warmupSwitch = await screen.findByRole("switch", {
      name: "settings.advanced.copilotOptimizer.warmupDowngrade",
    });
    expect(warmupSwitch).toHaveAttribute("aria-checked", "false");
    expect(
      screen.getByRole("switch", {
        name: "settings.advanced.copilotOptimizer.enabled",
      }),
    ).toHaveAttribute("aria-checked", "true");

    fireEvent.click(warmupSwitch);

    await waitFor(() => expect(setMock).toHaveBeenCalledTimes(1));
    expect(setMock).toHaveBeenCalledWith(
      expect.objectContaining({ warmupDowngrade: true, enabled: true }),
    );
  });

  it("saves the warmup model on blur and falls back to the default when cleared", async () => {
    getMock.mockResolvedValue({ ...DEFAULT_COPILOT_OPTIMIZER_CONFIG });
    render(<CopilotOptimizerPanel />);

    const input = (await screen.findByLabelText(
      "settings.advanced.copilotOptimizer.warmupModel",
    )) as HTMLInputElement;
    expect(input.value).toBe("gpt-5-mini");

    fireEvent.change(input, { target: { value: "gpt-4.1-mini" } });
    fireEvent.blur(input);
    await waitFor(() =>
      expect(setMock).toHaveBeenCalledWith(
        expect.objectContaining({ warmupModel: "gpt-4.1-mini" }),
      ),
    );

    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.blur(input);
    await waitFor(() =>
      expect(setMock).toHaveBeenLastCalledWith(
        expect.objectContaining({ warmupModel: "gpt-5-mini" }),
      ),
    );
    expect(input.value).toBe("gpt-5-mini");
  });

  it("disables the sub-switches while the master switch is off", async () => {
    getMock.mockResolvedValue({
      ...DEFAULT_COPILOT_OPTIMIZER_CONFIG,
      enabled: false,
    });
    render(<CopilotOptimizerPanel />);

    const classification = await screen.findByRole("switch", {
      name: "settings.advanced.copilotOptimizer.requestClassification",
    });
    expect(classification).toBeDisabled();
    expect(
      screen.getByLabelText("settings.advanced.copilotOptimizer.warmupModel"),
    ).toBeDisabled();
  });
});
