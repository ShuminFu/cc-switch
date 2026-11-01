import type { ReactNode } from "react";
import { Lightbulb } from "lucide-react";

/**
 * Remove a leading "💡" that older translations bake into hint strings, so
 * the icon is rendered as a decorative element instead of being announced
 * by screen readers and rendered inconsistently across platforms
 * (FE-PROV-15).
 */
export const stripHintEmoji = (text: string): string =>
  text.replace(/^\s*💡\s*/u, "");

/** A hint line with a decorative lightbulb icon. */
export function HintText({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <span className={className}>
      <Lightbulb
        className="mr-1 inline h-3.5 w-3.5 -translate-y-px"
        aria-hidden="true"
      />
      {typeof children === "string" ? stripHintEmoji(children) : children}
    </span>
  );
}
