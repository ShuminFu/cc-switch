import {
  useRef,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
} from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

/**
 * Single-select segmented control (FE-UI-21) shared by the Settings
 * pickers. Renders `role="radiogroup"` with `role="radio"` buttons, a
 * roving tab stop on the selected option, and arrow-key navigation
 * (Left/Up = previous, Right/Down = next, Home/End = first/last) that also
 * selects, like a native radio group.
 */
export interface SegmentedOption<T extends string> {
  value: T;
  label: ReactNode;
  icon?: React.ComponentType<{ className?: string }>;
  disabled?: boolean;
}

interface SegmentedControlProps<T extends string> {
  options: ReadonlyArray<SegmentedOption<T>>;
  value: T;
  onChange: (value: T, event?: MouseEvent<HTMLButtonElement>) => void;
  ariaLabel?: string;
  disabled?: boolean;
  className?: string;
  buttonClassName?: string;
}

export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  ariaLabel,
  disabled = false,
  className,
  buttonClassName,
}: SegmentedControlProps<T>) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const enabledIndexes = options
    .map((option, index) => (option.disabled || disabled ? -1 : index))
    .filter((index) => index >= 0);
  const selectedIndex = options.findIndex((option) => option.value === value);
  const tabStop = selectedIndex >= 0 ? selectedIndex : (enabledIndexes[0] ?? 0);

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (enabledIndexes.length === 0) return;
    const current = enabledIndexes.indexOf(
      refs.current.findIndex((el) => el === document.activeElement),
    );
    let next: number | null = null;
    switch (event.key) {
      case "ArrowRight":
      case "ArrowDown":
        next = enabledIndexes[(current + 1) % enabledIndexes.length];
        break;
      case "ArrowLeft":
      case "ArrowUp":
        next =
          enabledIndexes[
            (current - 1 + enabledIndexes.length) % enabledIndexes.length
          ];
        break;
      case "Home":
        next = enabledIndexes[0];
        break;
      case "End":
        next = enabledIndexes[enabledIndexes.length - 1];
        break;
      default:
        return;
    }
    event.preventDefault();
    if (next == null) return;
    refs.current[next]?.focus();
    if (options[next].value !== value) onChange(options[next].value);
  };

  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      onKeyDown={handleKeyDown}
      className={cn(
        "inline-flex gap-1 rounded-md border border-border-default bg-background p-1",
        className,
      )}
    >
      {options.map((option, index) => {
        const active = option.value === value;
        const Icon = option.icon;
        return (
          <Button
            key={option.value}
            ref={(el) => {
              refs.current[index] = el;
            }}
            type="button"
            role="radio"
            aria-checked={active}
            tabIndex={index === tabStop ? 0 : -1}
            disabled={disabled || option.disabled}
            onClick={(event) => {
              if (!active) onChange(option.value, event);
            }}
            size="sm"
            variant={active ? "default" : "ghost"}
            className={cn(
              "min-w-[96px]",
              Icon && "gap-1.5",
              active
                ? "shadow-sm"
                : "text-muted-foreground hover:text-foreground hover:bg-muted",
              buttonClassName,
            )}
          >
            {Icon && <Icon className="h-3.5 w-3.5" />}
            {option.label}
          </Button>
        );
      })}
    </div>
  );
}
