import { CircleHelp } from "lucide-react";
import { useTranslation } from "react-i18next";
import { settingsApi } from "@/lib/api";
import { helpUrl, type HelpTopic } from "@/lib/helpLinks";
import { cn } from "@/lib/utils";

interface HelpLinkProps {
  topic: HelpTopic;
  /** Optional heading anchor inside the manual page. */
  anchor?: string;
  className?: string;
}

/**
 * A "?" button for panel headers that opens the matching user-manual page in
 * the UI language, with English as fallback (DOCS-16).
 */
export function HelpLink({ topic, anchor, className }: HelpLinkProps) {
  const { t, i18n } = useTranslation();
  const label = t("helpLink.open", { defaultValue: "Open the manual" });
  return (
    <button
      type="button"
      className={cn(
        "inline-flex h-6 w-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground",
        className,
      )}
      aria-label={label}
      title={label}
      onClick={() => {
        void settingsApi
          .openExternal(
            helpUrl(topic, i18n.language ?? i18n.resolvedLanguage, anchor),
          )
          .catch((error: unknown) =>
            console.error("[HelpLink] failed to open the manual", error),
          );
      }}
    >
      <CircleHelp className="h-4 w-4" />
    </button>
  );
}
