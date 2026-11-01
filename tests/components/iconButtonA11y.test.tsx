import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Button } from "@/components/ui/button";
import { ProviderActions } from "@/components/providers/ProviderActions";

// FE-ARCH-20 / FE-PROV-14: icon-only buttons have an accessible name and
// unavailable actions are real disabled buttons, not just dimmed.
describe("icon button accessibility", () => {
  it("uses the tooltip as the accessible name of an icon button", () => {
    render(
      <Button size="icon" title="Refresh">
        <svg />
      </Button>,
    );
    expect(screen.getByRole("button", { name: "Refresh" })).toBeInTheDocument();
  });

  it("keeps an explicit aria-label", () => {
    render(
      <Button size="icon" title="Tooltip" aria-label="Explicit">
        <svg />
      </Button>,
    );
    expect(
      screen.getByRole("button", { name: "Explicit" }),
    ).toBeInTheDocument();
  });

  it("does not label text buttons from their title", () => {
    render(<Button title="Tip">Save</Button>);
    expect(screen.getByRole("button", { name: "Save" })).toBeInTheDocument();
  });

  it("disables unavailable provider actions", () => {
    render(
      <ProviderActions
        appId="claude"
        isCurrent
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDuplicate={vi.fn()}
        onDelete={vi.fn()}
      />,
    );
    // No onTest / onConfigureUsage handlers and the current provider cannot
    // be deleted.
    expect(
      screen.getByRole("button", { name: "Connectivity check" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Configure usage query" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete" })).toBeDisabled();
  });
});
