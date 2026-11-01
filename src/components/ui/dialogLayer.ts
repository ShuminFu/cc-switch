import * as React from "react";

export type DialogLayer = "base" | "nested" | "alert" | "top";

/** One z-index map for overlay and content (FE-UI-47). */
export const Z_LAYERS: Record<DialogLayer, string> = {
  base: "z-40",
  nested: "z-50",
  alert: "z-[60]",
  top: "z-[110]",
};

const LAYER_ORDER: DialogLayer[] = ["base", "nested", "alert", "top"];

/**
 * Minimum layer for dialogs rendered inside a layered surface. FullScreenPanel
 * (z-[60]) provides "alert", so a dialog opened from a full-screen panel
 * renders above it without every caller passing `zIndex="alert"`.
 */
export const DialogLayerContext = React.createContext<DialogLayer>("base");

export const nextLayer = (layer: DialogLayer): DialogLayer =>
  LAYER_ORDER[Math.min(LAYER_ORDER.indexOf(layer) + 1, LAYER_ORDER.length - 1)];

export const resolveLayer = (
  requested: DialogLayer | undefined,
  min: DialogLayer,
) => {
  const wanted = requested ?? "base";
  return LAYER_ORDER.indexOf(wanted) >= LAYER_ORDER.indexOf(min) ? wanted : min;
};
