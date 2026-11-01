import { ArrowDown, ArrowUp } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";

/** Return a copy of `items` with the element at `from` moved to `to`. */
export function moveItem<T>(items: readonly T[], from: number, to: number) {
  const next = [...items];
  if (to < 0 || to >= next.length || from === to) return next;
  const [item] = next.splice(from, 1);
  next.splice(to, 0, item);
  return next;
}

interface ModelOrderButtonsProps {
  index: number;
  count: number;
  onMove: (from: number, to: number) => void;
}

/**
 * Move-up / move-down controls for ordered model lists, where the first
 * entry is the default model (FE-PROV-11).
 */
export function ModelOrderButtons({
  index,
  count,
  onMove,
}: ModelOrderButtonsProps) {
  const { t } = useTranslation();
  return (
    <div className="mt-5 flex flex-col">
      <Button
        type="button"
        variant="ghost"
        size="icon"
        className="h-4 w-7 text-muted-foreground"
        disabled={index === 0}
        onClick={() => onMove(index, index - 1)}
        aria-label={t("providerForm.moveModelUp", {
          defaultValue: "Move model up",
        })}
        title={t("providerForm.moveModelUp", {
          defaultValue: "Move model up",
        })}
      >
        <ArrowUp className="h-3.5 w-3.5" />
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="icon"
        className="h-4 w-7 text-muted-foreground"
        disabled={index >= count - 1}
        onClick={() => onMove(index, index + 1)}
        aria-label={t("providerForm.moveModelDown", {
          defaultValue: "Move model down",
        })}
        title={t("providerForm.moveModelDown", {
          defaultValue: "Move model down",
        })}
      >
        <ArrowDown className="h-3.5 w-3.5" />
      </Button>
    </div>
  );
}
