import * as React from "react";
import { useTranslation } from "react-i18next";
import { Input, type InputProps } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { parseStrictInteger } from "@/utils/numberInput";

export interface IntegerFieldProps
  extends Omit<
    InputProps,
    "value" | "defaultValue" | "onChange" | "type" | "min" | "max" | "step"
  > {
  /** Current value; `undefined` means empty. */
  value: number | undefined;
  /**
   * Called with the parsed integer, or `undefined` when the field is cleared.
   * Not called while the text is invalid — the last valid value is kept and
   * an inline message is shown instead.
   */
  onValueChange: (value: number | undefined) => void;
  /** Smallest accepted value (default 0, i.e. negatives are rejected). */
  min?: number;
  /** Largest accepted value (default: none). */
  max?: number;
  /** Increment used by ArrowUp / ArrowDown (default 1). */
  step?: number;
  /** Notified whenever the text switches between valid and invalid. */
  onValidityChange?: (valid: boolean) => void;
  /** Extra classes for the wrapper element. */
  wrapperClassName?: string;
}

type Validation =
  | { ok: true; value: number | undefined }
  | { ok: false; message: string };

/**
 * Integer-only text input with inline validation.
 *
 * Uses `type="text"` + `inputMode="numeric"` rather than `type="number"`:
 * number inputs report "" for partial/invalid text such as "1e" or "12abc",
 * which makes it impossible to tell "cleared" from "invalid". Values are
 * parsed strictly (decimals, garbage and out-of-range values are rejected,
 * never coerced) via `parseStrictInteger`.
 */
export const IntegerField = React.forwardRef<
  HTMLInputElement,
  IntegerFieldProps
>(function IntegerField(
  {
    value,
    onValueChange,
    min = 0,
    max,
    step = 1,
    onValidityChange,
    wrapperClassName,
    className,
    id,
    onKeyDown,
    ...inputProps
  },
  ref,
) {
  const { t } = useTranslation();
  const generatedId = React.useId();
  const inputId = id ?? generatedId;
  const errorId = `${inputId}-error`;

  const [text, setText] = React.useState(() =>
    value === undefined ? "" : String(value),
  );

  const validate = React.useCallback(
    (raw: string): Validation => {
      const parsed = parseStrictInteger(raw);
      if (parsed === undefined) return { ok: true, value: undefined };
      if (parsed === null)
        return { ok: false, message: t("common.integerInvalid") };
      if (parsed < min)
        return { ok: false, message: t("common.integerMin", { min }) };
      if (max !== undefined && parsed > max)
        return { ok: false, message: t("common.integerMax", { max }) };
      return { ok: true, value: parsed };
    },
    [min, max, t],
  );

  const validation = validate(text);

  // Follow external value changes (form reset, preset applied, …) without
  // overwriting text the user is in the middle of typing.
  const lastValueRef = React.useRef(value);
  React.useEffect(() => {
    if (Object.is(lastValueRef.current, value)) return;
    lastValueRef.current = value;
    setText((current) => {
      const parsed = parseStrictInteger(current);
      if (parsed === value || (parsed === undefined && value === undefined)) {
        return current;
      }
      return value === undefined ? "" : String(value);
    });
  }, [value]);

  const lastValidityRef = React.useRef(validation.ok);
  React.useEffect(() => {
    if (lastValidityRef.current === validation.ok) return;
    lastValidityRef.current = validation.ok;
    onValidityChange?.(validation.ok);
  }, [validation.ok, onValidityChange]);

  const commit = (raw: string) => {
    setText(raw);
    const result = validate(raw);
    if (result.ok) {
      lastValueRef.current = result.value;
      onValueChange(result.value);
    }
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    onKeyDown?.(event);
    if (event.defaultPrevented) return;
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    const base = validation.ok ? (validation.value ?? min) : (value ?? min);
    let next = base + (event.key === "ArrowUp" ? step : -step);
    if (next < min) next = min;
    if (max !== undefined && next > max) next = max;
    commit(String(next));
  };

  return (
    <div className={cn("space-y-1", wrapperClassName)}>
      <Input
        {...inputProps}
        ref={ref}
        id={inputId}
        type="text"
        inputMode="numeric"
        value={text}
        onChange={(event) => commit(event.target.value)}
        onKeyDown={handleKeyDown}
        aria-invalid={!validation.ok || undefined}
        aria-describedby={
          validation.ok
            ? inputProps["aria-describedby"]
            : cn(inputProps["aria-describedby"], errorId)
        }
        className={cn(
          !validation.ok &&
            "border-destructive focus:ring-destructive/20 dark:focus:ring-destructive/20",
          className,
        )}
      />
      {!validation.ok && (
        <p id={errorId} className="text-xs text-destructive">
          {validation.message}
        </p>
      )}
    </div>
  );
});
