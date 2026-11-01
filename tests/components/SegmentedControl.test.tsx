import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { SegmentedControl } from "@/components/common/SegmentedControl";

function Harness({ onChange }: { onChange?: (value: string) => void }) {
  const [value, setValue] = useState("b");
  return (
    <SegmentedControl
      ariaLabel="Pick"
      value={value}
      onChange={(next) => {
        setValue(next);
        onChange?.(next);
      }}
      options={[
        { value: "a", label: "Alpha" },
        { value: "b", label: "Bravo" },
        { value: "c", label: "Charlie", disabled: true },
        { value: "d", label: "Delta" },
      ]}
    />
  );
}

describe("SegmentedControl (FE-UI-21)", () => {
  it("exposes radiogroup semantics with a roving tab stop", () => {
    render(<Harness />);
    expect(
      screen.getByRole("radiogroup", { name: "Pick" }),
    ).toBeInTheDocument();
    const bravo = screen.getByRole("radio", { name: "Bravo" });
    expect(bravo).toHaveAttribute("aria-checked", "true");
    expect(bravo).toHaveAttribute("tabindex", "0");
    expect(screen.getByRole("radio", { name: "Alpha" })).toHaveAttribute(
      "tabindex",
      "-1",
    );
  });

  it("moves and selects with arrow keys, skipping disabled options", () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    const bravo = screen.getByRole("radio", { name: "Bravo" });
    bravo.focus();

    fireEvent.keyDown(bravo, { key: "ArrowRight" });
    const delta = screen.getByRole("radio", { name: "Delta" });
    expect(document.activeElement).toBe(delta);
    expect(delta).toHaveAttribute("aria-checked", "true");
    expect(onChange).toHaveBeenLastCalledWith("d");

    fireEvent.keyDown(delta, { key: "ArrowRight" });
    expect(onChange).toHaveBeenLastCalledWith("a");
    fireEvent.keyDown(document.activeElement!, { key: "End" });
    expect(onChange).toHaveBeenLastCalledWith("d");
  });

  it("selects on click and ignores clicks on the active option", () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    fireEvent.click(screen.getByRole("radio", { name: "Bravo" }));
    expect(onChange).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("radio", { name: "Alpha" }));
    expect(onChange).toHaveBeenCalledWith("a");
  });
});
