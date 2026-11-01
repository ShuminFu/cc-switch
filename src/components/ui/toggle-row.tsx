import { useId } from "react";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";

export interface ToggleRowProps {
  icon: React.ReactNode;
  title: string;
  description?: string;
  checked: boolean;
  onCheckedChange: (value: boolean) => void;
  disabled?: boolean;
}

export function ToggleRow({
  icon,
  title,
  description,
  checked,
  onCheckedChange,
  disabled,
}: ToggleRowProps) {
  const id = useId();
  const descriptionId = description ? `${id}-description` : undefined;
  // The whole row is the switch's <label>, so clicking the title or the
  // description toggles it (FE-UI-40).
  return (
    <label
      htmlFor={id}
      className={cn(
        "flex items-center justify-between gap-4 rounded-xl border border-border bg-card/50 p-4 transition-colors hover:bg-muted/50",
        disabled ? "cursor-not-allowed opacity-60" : "cursor-pointer",
      )}
    >
      <div className="flex items-center gap-3">
        <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-background ring-1 ring-border">
          {icon}
        </div>
        <div className="space-y-1">
          <p className="text-sm font-medium leading-none">{title}</p>
          {description ? (
            <p id={descriptionId} className="text-xs text-muted-foreground">
              {description}
            </p>
          ) : null}
        </div>
      </div>
      <Switch
        id={id}
        checked={checked}
        onCheckedChange={onCheckedChange}
        disabled={disabled}
        aria-label={title}
        aria-describedby={descriptionId}
      />
    </label>
  );
}
