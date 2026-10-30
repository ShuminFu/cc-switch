import {
  createContext,
  useContext,
  useRef,
  type CSSProperties,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { useAutoCompact } from "@/hooks/useAutoCompact";

/**
 * Header slots owned by the shell and filled by pages (FE-ARCH-02).
 *
 * The shell renders the header chrome and two empty containers; a page
 * declares its own header content with `<PageHeaderStart>` (left, used by the
 * root page in place of the back button + title) and `<PageHeaderActions>`
 * (right). This replaces the shell calling into pages through `useRef<any>`
 * imperative handles.
 */
export interface PageHeaderSlots {
  start: HTMLElement | null;
  actions: HTMLElement | null;
}

const PageHeaderContext = createContext<PageHeaderSlots>({
  start: null,
  actions: null,
});

export const PageHeaderSlotsProvider = PageHeaderContext.Provider;

/** `-webkit-app-region` is not in React's CSSProperties. */
export const NO_DRAG_STYLE = { WebkitAppRegion: "no-drag" } as CSSProperties;

export function PageHeaderStart({ children }: { children: ReactNode }) {
  const { start } = useContext(PageHeaderContext);
  return start ? createPortal(children, start) : null;
}

export function PageHeaderActions({ children }: { children: ReactNode }) {
  const { actions } = useContext(PageHeaderContext);
  return actions ? createPortal(children, actions) : null;
}

/**
 * The flexible right-aligned toolbar area. Children get the measured
 * `compact` flag so they can collapse labels when the header is narrow.
 */
export function HeaderToolbar({
  children,
}: {
  children: ReactNode | ((compact: boolean) => ReactNode);
}) {
  const toolbarRef = useRef<HTMLDivElement>(null);
  const compact = useAutoCompact(toolbarRef);
  return (
    <div
      ref={toolbarRef}
      className="flex flex-1 min-w-0 overflow-x-hidden items-center py-4 pr-2"
    >
      <div
        className="flex shrink-0 items-center gap-1.5 ml-auto"
        style={NO_DRAG_STYLE}
      >
        {typeof children === "function" ? children(compact) : children}
      </div>
    </div>
  );
}
