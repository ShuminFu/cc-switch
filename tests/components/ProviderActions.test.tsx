import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderActions } from "@/components/providers/ProviderActions";

function renderActions(
  overrides: Partial<Parameters<typeof ProviderActions>[0]> = {},
) {
  const props = {
    appId: "claude" as const,
    isCurrent: false,
    onSwitch: vi.fn(),
    onEdit: vi.fn(),
    onDuplicate: vi.fn(),
    onDelete: vi.fn(),
    ...overrides,
  };
  render(<ProviderActions {...props} />);
  return props;
}

describe("ProviderActions", () => {
  it("offers 'copy as shell profile' only when the provider id is known", () => {
    renderActions({ providerId: "p1" });
    expect(
      screen.getByRole("button", {
        name: /env\.profile\.copy|Copy as shell profile/,
      }),
    ).toBeInTheDocument();
  });

  it("hides 'copy as shell profile' without a provider id", () => {
    renderActions();
    expect(
      screen.queryByRole("button", {
        name: /env\.profile\.copy|Copy as shell profile/,
      }),
    ).toBeNull();
  });

  it("shows a disabled pending button instead of Add while live ids load (FE-PROV-51)", () => {
    const props = renderActions({
      appId: "openclaw",
      isInConfig: false,
      isConfigStatusLoading: true,
      onSetAsDefault: vi.fn(),
    });
    expect(
      screen.queryByRole("button", { name: /provider.addToConfig/ }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: /provider.setAsDefault/ }),
    ).toBeNull();
    const pending = screen.getByRole("button", {
      name: /configStatusLoading|Checking the app config/,
    });
    expect(pending).toBeDisabled();
    fireEvent.click(pending);
    expect(props.onSwitch).not.toHaveBeenCalled();
  });

  it("disables Duplicate for read-only (Hermes-managed) providers (FE-PROV-42)", () => {
    const props = renderActions({ appId: "hermes", isReadOnly: true });

    const hint =
      "Defined in Hermes' providers: dict. Edit or remove it via Hermes Web UI.";
    // Edit, Duplicate and Delete all carry the managed hint when read-only.
    const buttons = screen.getAllByTitle(hint);
    expect(buttons).toHaveLength(3);
    // Edit and Duplicate are real disabled buttons.
    expect(buttons[0]).toBeDisabled();
    expect(buttons[1]).toBeDisabled();

    for (const button of buttons) {
      fireEvent.click(button);
    }
    expect(props.onDuplicate).not.toHaveBeenCalled();
    expect(props.onEdit).not.toHaveBeenCalled();
    expect(props.onDelete).not.toHaveBeenCalled();
    expect(screen.queryByTitle("Duplicate")).toBeNull();
  });

  it("keeps Duplicate enabled for normal providers", () => {
    const props = renderActions();
    const duplicate = screen.getByTitle("Duplicate");
    expect(duplicate).not.toBeDisabled();
    fireEvent.click(duplicate);
    expect(props.onDuplicate).toHaveBeenCalledTimes(1);
  });

  it("disables the main button while a provider mutation is pending (FE-PROV-49)", () => {
    const props = renderActions({ isActionPending: true });
    const main = screen.getByRole("button", { name: /Enable/ });
    expect(main).toBeDisabled();
    fireEvent.click(main);
    expect(props.onSwitch).not.toHaveBeenCalled();
  });

  it("allows switching when nothing is pending", () => {
    const props = renderActions();
    fireEvent.click(screen.getByRole("button", { name: /Enable/ }));
    expect(props.onSwitch).toHaveBeenCalledTimes(1);
  });
});
