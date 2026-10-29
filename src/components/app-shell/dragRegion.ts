import type { CSSProperties } from "react";
import { DRAG_REGION_STYLE } from "@/lib/platform";

/** `WebkitAppRegion` 不在 React 的 CSSProperties 中，这里补充类型 */
export type AppRegionStyle = CSSProperties & {
  WebkitAppRegion?: "drag" | "no-drag";
};

/** 拖拽区域内的可交互元素需显式标记为 no-drag */
export const NO_DRAG_STYLE: AppRegionStyle = { WebkitAppRegion: "no-drag" };

/** 平台相关的拖拽样式（Linux 下为空对象，见 `DRAG_REGION_STYLE`） */
export const PLATFORM_DRAG_STYLE = DRAG_REGION_STYLE as AppRegionStyle;
