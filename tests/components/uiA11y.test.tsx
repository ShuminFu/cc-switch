import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ToggleRow } from "@/components/ui/toggle-row";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { toast } from "sonner";
import { ERROR_TOAST_DURATION_MS } from "@/components/ui/sonner";

describe("FullScreenPanel dialog semantics (FE-UI-12)", () => {
  it("is a named modal dialog and moves focus inside on open", async () => {
    const opener = document.createElement("button");
    document.body.appendChild(opener);
    opener.focus();

    const { rerender } = render(
      <FullScreenPanel isOpen title="Edit thing" onClose={vi.fn()}>
        <p>body</p>
      </FullScreenPanel>,
    );

    const dialog = screen.getByRole("dialog", { name: "Edit thing" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    await waitFor(() =>
      expect(dialog.contains(document.activeElement)).toBe(true),
    );

    rerender(
      <FullScreenPanel isOpen={false} title="Edit thing" onClose={vi.fn()}>
        <p>body</p>
      </FullScreenPanel>,
    );
    await waitFor(() => expect(document.activeElement).toBe(opener));
    opener.remove();
  });

  it("stacks dialogs opened inside it above the panel (FE-UI-47)", () => {
    render(
      <FullScreenPanel isOpen title="Panel" onClose={vi.fn()}>
        <Dialog open>
          <DialogContent>
            <DialogTitle>Inner</DialogTitle>
          </DialogContent>
        </Dialog>
      </FullScreenPanel>,
    );
    const inner = screen.getByRole("dialog", { name: "Inner" });
    expect(inner.className).toContain("z-[60]");
  });
});

describe("Dialog outside-click dismissal (FE-UI-12)", () => {
  // Radix registers its outside-pointer listener on the next tick.
  const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

  it("is opt-in per dialog", async () => {
    const onOpenChange = vi.fn();
    render(
      <Dialog open onOpenChange={onOpenChange}>
        <DialogContent dismissOnOutsideClick>
          <DialogTitle>Info</DialogTitle>
        </DialogContent>
      </Dialog>,
    );
    await tick();
    fireEvent.pointerDown(document.body);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("stays open on outside click by default", async () => {
    const onOpenChange = vi.fn();
    render(
      <Dialog open onOpenChange={onOpenChange}>
        <DialogContent>
          <DialogTitle>Form</DialogTitle>
        </DialogContent>
      </Dialog>,
    );
    await tick();
    fireEvent.pointerDown(document.body);
    expect(onOpenChange).not.toHaveBeenCalled();
  });
});

describe("ToggleRow (FE-UI-40)", () => {
  it("toggles when the title is clicked and links the description", () => {
    const onChange = vi.fn();
    render(
      <ToggleRow
        icon={null}
        title="Launch at login"
        description="Start with the OS"
        checked={false}
        onCheckedChange={onChange}
      />,
    );
    fireEvent.click(screen.getByText("Launch at login"));
    expect(onChange).toHaveBeenCalledWith(true);

    const toggle = screen.getByRole("switch", { name: "Launch at login" });
    expect(toggle).toHaveAccessibleDescription("Start with the OS");
  });
});

describe("error toast duration (FE-UI-13)", () => {
  it("keeps error toasts up for at least 6 s unless a duration is given", () => {
    toast.error("boom", { description: "details" });
    toast.error("quick", { duration: 1000 });
    const history = toast.getHistory() as Array<{
      title?: unknown;
      duration?: number;
    }>;
    const boom = history.find((t) => t.title === "boom");
    const quick = history.find((t) => t.title === "quick");
    expect(ERROR_TOAST_DURATION_MS).toBeGreaterThanOrEqual(6000);
    expect(boom?.duration).toBe(ERROR_TOAST_DURATION_MS);
    expect(quick?.duration).toBe(1000);
  });
});
