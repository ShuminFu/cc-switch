import { useEffect, useRef, useSyncExternalStore } from "react";
import { isTextEditableTarget } from "@/utils/domUtils";

/**
 * Keyboard shortcut registry (FE-ARCH-16). Components register shortcuts
 * with `useShortcut`; one window listener dispatches them, so text-field
 * handling and modifier parsing live in one place, and the registered list
 * can be shown to the user (`useRegisteredShortcuts`).
 */

export interface ShortcutDefinition {
  /** Stable id, e.g. "palette.open". */
  id: string;
  /**
   * Key combination: `mod+k` (mod = Cmd on macOS, Ctrl elsewhere),
   * `mod+,`, `mod+f`, `escape`, `shift+?`.
   */
  keys: string;
  /** Human readable label for the shortcut overlay. */
  label: string;
  /** Fire even while a text field has focus (default: only with a modifier). */
  allowInInput?: boolean;
  /** Return false to let the event continue to other handlers. */
  handler: (event: KeyboardEvent) => void | boolean;
}

interface ParsedKeys {
  mod: boolean;
  shift: boolean;
  alt: boolean;
  key: string;
}

export function parseKeys(keys: string): ParsedKeys {
  const parts = keys.toLowerCase().split("+");
  // "mod++" style (plus key) is not used; the last part is the key.
  const key = parts[parts.length - 1];
  return {
    mod: parts.includes("mod"),
    shift: parts.includes("shift"),
    alt: parts.includes("alt"),
    key,
  };
}

export function matchesShortcut(event: KeyboardEvent, keys: string): boolean {
  const parsed = parseKeys(keys);
  const mod = event.metaKey || event.ctrlKey;
  if (parsed.mod !== mod) return false;
  if (parsed.alt !== event.altKey) return false;
  // Shift is implied by characters such as "?" on most layouts.
  if (parsed.shift && !event.shiftKey) return false;
  return event.key.toLowerCase() === parsed.key;
}

const registry = new Map<string, ShortcutDefinition>();
const listeners = new Set<() => void>();
let snapshot: ShortcutDefinition[] = [];

function emit() {
  snapshot = Array.from(registry.values());
  listeners.forEach((listener) => listener());
}

function handleKeyDown(event: KeyboardEvent) {
  if (event.defaultPrevented) return;
  // Later registrations win (a page shortcut overrides a shell one).
  const entries = Array.from(registry.values()).reverse();
  for (const shortcut of entries) {
    if (!matchesShortcut(event, shortcut.keys)) continue;
    const hasModifier = parseKeys(shortcut.keys).mod;
    if (
      !shortcut.allowInInput &&
      !hasModifier &&
      isTextEditableTarget(event.target)
    ) {
      continue;
    }
    const result = shortcut.handler(event);
    if (result === false) continue;
    event.preventDefault();
    return;
  }
}

let attached = false;
function ensureListener() {
  if (attached || typeof window === "undefined") return;
  window.addEventListener("keydown", handleKeyDown);
  attached = true;
}

export function registerShortcut(definition: ShortcutDefinition): () => void {
  ensureListener();
  registry.set(definition.id, definition);
  emit();
  return () => {
    if (registry.get(definition.id) === definition) {
      registry.delete(definition.id);
      emit();
    }
  };
}

/**
 * Register a shortcut for the lifetime of the component. The latest
 * `handler` is always called; `enabled: false` unregisters it.
 */
export function useShortcut(
  definition: Omit<ShortcutDefinition, "handler">,
  handler: ShortcutDefinition["handler"],
  enabled = true,
): void {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;
  const { id, keys, label, allowInInput } = definition;

  useEffect(() => {
    if (!enabled) return;
    return registerShortcut({
      id,
      keys,
      label,
      allowInInput,
      handler: (event) => handlerRef.current(event),
    });
  }, [id, keys, label, allowInInput, enabled]);
}

/** Currently registered shortcuts (for a help overlay or the palette). */
export function useRegisteredShortcuts(): ShortcutDefinition[] {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => snapshot,
    () => snapshot,
  );
}

/** Display form of a key combination for the current platform. */
export function formatShortcut(keys: string, isMac: boolean): string {
  return keys
    .split("+")
    .map((part) => {
      const lower = part.toLowerCase();
      if (lower === "mod") return isMac ? "⌘" : "Ctrl";
      if (lower === "shift") return isMac ? "⇧" : "Shift";
      if (lower === "alt") return isMac ? "⌥" : "Alt";
      if (lower === "escape") return "Esc";
      return part.length === 1 ? part.toUpperCase() : part;
    })
    .join(isMac ? "" : "+");
}
