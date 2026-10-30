import type { ComponentPropsWithoutRef, ReactNode } from "react";
import { Loader2 } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { ActionState } from "./types";

/** Button that shows a spinner while its action runs and locks while any runs. */
export function ActionButton({
  actionState,
  targetState,
  alsoActiveFor,
  icon: Icon,
  activeLabel,
  idleLabel,
  disabled,
  ...props
}: {
  actionState: ActionState;
  targetState: ActionState;
  alsoActiveFor?: ActionState[];
  icon: LucideIcon;
  activeLabel: ReactNode;
  idleLabel: ReactNode;
} & Omit<ComponentPropsWithoutRef<typeof Button>, "children">) {
  const isActive =
    actionState === targetState ||
    (alsoActiveFor?.includes(actionState) ?? false);
  return (
    <Button {...props} disabled={actionState !== "idle" || disabled}>
      <span className="inline-flex items-center gap-2">
        {isActive ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
        ) : (
          <Icon className="h-3.5 w-3.5" />
        )}
        {isActive ? activeLabel : idleLabel}
      </span>
    </Button>
  );
}
