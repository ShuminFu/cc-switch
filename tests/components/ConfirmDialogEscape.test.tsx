import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ExternalChangeBanner } from "@/components/common/ExternalChangeBanner";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("ConfirmDialog", () => {
  it("consumes ESC so window-level ESC handlers underneath don't fire", () => {
    const onCancel = vi.fn();
    const windowEsc = vi.fn();
    window.addEventListener("keydown", windowEsc);

    try {
      render(
        <ConfirmDialog
          isOpen
          title="title"
          message="message"
          onConfirm={vi.fn()}
          onCancel={onCancel}
        />,
      );

      fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

      expect(onCancel).toHaveBeenCalledTimes(1);
      expect(windowEsc).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener("keydown", windowEsc);
    }
  });
});

describe("ExternalChangeBanner", () => {
  it("wires the Reload and Keep mine actions", () => {
    const onReload = vi.fn();
    const onKeepMine = vi.fn();
    render(
      <ExternalChangeBanner onReload={onReload} onKeepMine={onKeepMine} />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent(
      "common.externalChangeMessage",
    );
    fireEvent.click(screen.getByText("common.externalChangeReload"));
    fireEvent.click(screen.getByText("common.externalChangeKeepMine"));
    expect(onReload).toHaveBeenCalledTimes(1);
    expect(onKeepMine).toHaveBeenCalledTimes(1);
  });
});
