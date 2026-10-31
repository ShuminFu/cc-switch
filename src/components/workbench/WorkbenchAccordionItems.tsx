import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { History } from "lucide-react";
import {
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import { ConfigHistoryPanel } from "./ConfigHistoryPanel";
import { AuditLogPanel } from "./AuditLogPanel";

function Item({
  value,
  icon,
  title,
  description,
  children,
}: {
  value: string;
  icon: ReactNode;
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <AccordionItem
      value={value}
      className="rounded-xl glass-card overflow-hidden"
    >
      <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
        <div className="flex items-center gap-3">
          {icon}
          <div className="text-left">
            <h3 className="text-base font-semibold">{title}</h3>
            <p className="text-sm text-muted-foreground font-normal">
              {description}
            </p>
          </div>
        </div>
      </AccordionTrigger>
      <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
        {children}
      </AccordionContent>
    </AccordionItem>
  );
}

/**
 * Settings → Advanced sections for the workbench features. Rendered inside
 * the page's existing `<Accordion>` (Radix items use context, so a wrapper
 * component is fine).
 */
export function WorkbenchAccordionItems() {
  const { t } = useTranslation();
  return (
    <>
      <Item
        value="history"
        icon={<History className="h-5 w-5 text-sky-500" />}
        title={t("workbench.history.title", {
          defaultValue: "Change history & audit log",
        })}
        description={t("workbench.history.subtitle", {
          defaultValue: "See what changed, where it came from, and roll back",
        })}
      >
        <div className="space-y-6">
          <ConfigHistoryPanel />
          <AuditLogPanel />
        </div>
      </Item>
    </>
  );
}
