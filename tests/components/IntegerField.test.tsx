import { useState } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  IntegerField,
  type IntegerFieldProps,
} from "@/components/common/IntegerField";

vi.mock("react-i18next", () => {
  const t = (key: string, opts?: Record<string, unknown>) =>
    opts ? `${key}:${JSON.stringify(opts)}` : key;
  return { useTranslation: () => ({ t }) };
});

function Harness({
  initial,
  onValueChange,
  ...props
}: Partial<IntegerFieldProps> & {
  initial?: number;
  onValueChange?: (v: number | undefined) => void;
}) {
  const [value, setValue] = useState<number | undefined>(initial);
  return (
    <>
      <IntegerField
        aria-label="field"
        {...props}
        value={value}
        onValueChange={(v) => {
          setValue(v);
          onValueChange?.(v);
        }}
      />
      <output data-testid="value">{String(value)}</output>
      <button type="button" onClick={() => setValue(42)}>
        external
      </button>
    </>
  );
}

const input = () => screen.getByLabelText("field") as HTMLInputElement;
const type = (text: string) =>
  fireEvent.change(input(), { target: { value: text } });

describe("IntegerField", () => {
  it("shows the initial value and reports parsed integers", () => {
    const onValueChange = vi.fn();
    render(<Harness initial={7} onValueChange={onValueChange} />);
    expect(input()).toHaveValue("7");

    type("1e5");
    expect(onValueChange).toHaveBeenLastCalledWith(100000);
    expect(screen.getByTestId("value")).toHaveTextContent("100000");
  });

  it("maps an empty field to undefined", () => {
    const onValueChange = vi.fn();
    render(<Harness initial={7} onValueChange={onValueChange} />);
    type("");
    expect(onValueChange).toHaveBeenLastCalledWith(undefined);
    expect(input()).not.toHaveAttribute("aria-invalid");
  });

  it.each(["1.5", "12abc", "0x10", "--1"])(
    "rejects %s inline and keeps the last valid value",
    (text) => {
      const onValueChange = vi.fn();
      render(<Harness initial={7} onValueChange={onValueChange} />);
      type(text);

      expect(onValueChange).not.toHaveBeenCalled();
      expect(input()).toHaveValue(text);
      expect(input()).toHaveAttribute("aria-invalid", "true");
      expect(screen.getByText("common.integerInvalid")).toBeInTheDocument();
      expect(screen.getByTestId("value")).toHaveTextContent("7");
    },
  );

  it("rejects negatives by default (min 0)", () => {
    render(<Harness initial={7} />);
    type("-1");
    expect(screen.getByText('common.integerMin:{"min":0}')).toBeInTheDocument();
  });

  it("honours min / max props", () => {
    const onValueChange = vi.fn();
    render(<Harness min={-5} max={10} onValueChange={onValueChange} />);
    type("-3");
    expect(onValueChange).toHaveBeenLastCalledWith(-3);
    type("11");
    expect(
      screen.getByText('common.integerMax:{"max":10}'),
    ).toBeInTheDocument();
    type("-6");
    expect(
      screen.getByText('common.integerMin:{"min":-5}'),
    ).toBeInTheDocument();
  });

  it("steps with ArrowUp / ArrowDown, clamped to the range", () => {
    render(<Harness initial={9} max={10} step={5} />);
    fireEvent.keyDown(input(), { key: "ArrowUp" });
    expect(input()).toHaveValue("10");
    fireEvent.keyDown(input(), { key: "ArrowDown" });
    expect(input()).toHaveValue("5");
    fireEvent.keyDown(input(), { key: "ArrowDown" });
    fireEvent.keyDown(input(), { key: "ArrowDown" });
    expect(input()).toHaveValue("0");
  });

  it("follows external value changes", () => {
    render(<Harness initial={1} />);
    type("abc");
    fireEvent.click(screen.getByText("external"));
    expect(input()).toHaveValue("42");
    expect(input()).not.toHaveAttribute("aria-invalid");
  });

  it("reports validity transitions", () => {
    const onValidityChange = vi.fn();
    render(<Harness initial={1} onValidityChange={onValidityChange} />);
    type("1.");
    type("1.5");
    type("2");
    expect(onValidityChange.mock.calls).toEqual([[false], [true]]);
  });
});
