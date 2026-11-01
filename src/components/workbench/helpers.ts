import type { AuditEvent, BundlePreview } from "./api";

/** Pure helpers shared by the workbench panels (kept out of component files). */

export const matchesAuditFilter = (event: AuditEvent, filter: string) => {
  const q = filter.trim().toLowerCase();
  if (!q) return true;
  return [event.actor, event.action, event.entityId, event.app, event.detail]
    .filter(Boolean)
    .some((v) => String(v).toLowerCase().includes(q));
};

/** Items selected by default: everything except identical copies. */
export const defaultSelection = (preview: BundlePreview) =>
  new Set(
    preview.items.filter((i) => i.status !== "identical").map((i) => i.key),
  );

/** Last segment of a JSON pointer, for labelling a missing secret. */
export const pointerLabel = (pointer: string) =>
  pointer.split("/").filter(Boolean).pop()?.replace(/~1/g, "/") ?? pointer;
