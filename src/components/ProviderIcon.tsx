import React, { useMemo } from "react";
import {
  getIcon,
  hasIcon,
  getIconMetadata,
  getIconUrl,
  isUrlIcon,
} from "@/icons/extracted";
import { cn } from "@/lib/utils";

interface ProviderIconProps {
  icon?: string; // 图标名称
  name: string; // 供应商名称（用于 fallback）
  color?: string; // 自定义颜色 (Deprecated, kept for compatibility but ignored for SVG)
  size?: number | string; // 尺寸
  className?: string;
  showFallback?: boolean; // 是否显示 fallback
}

const svgDataUrl = (svg: string) =>
  `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;

/** Neutral colour for `currentColor` parts of multi-colour icons. */
const MIXED_ICON_FALLBACK_COLOR = "#8a8a8a";

/**
 * How to draw a bundled SVG icon without inline markup: as a mask when it is
 * drawn only in `currentColor` (so it follows the CSS text colour), otherwise
 * as an image, with any `currentColor` parts resolved to `color`.
 */
export function svgIconRendering(
  svg: string,
  color?: string,
): { kind: "mask" | "image"; src: string } {
  const usesCurrentColor = svg.includes("currentColor");
  const hasOwnColors = /(?:fill|stroke|stop-color)\s*[=:]\s*"?#|url\(#/.test(
    svg,
  );
  if (usesCurrentColor && !hasOwnColors) {
    return { kind: "mask", src: svgDataUrl(svg) };
  }
  const resolved = usesCurrentColor
    ? svg.split("currentColor").join(color ?? MIXED_ICON_FALLBACK_COLOR)
    : svg;
  return { kind: "image", src: svgDataUrl(resolved) };
}

export const ProviderIcon: React.FC<ProviderIconProps> = ({
  icon,
  name,
  color,
  size = 32,
  className,
  showFallback = true,
}) => {
  // 获取内联 SVG 字符串
  const iconSvg = useMemo(() => {
    if (icon && !isUrlIcon(icon) && hasIcon(icon)) {
      return getIcon(icon);
    }
    return "";
  }, [icon]);

  // 获取图标 URL（URL_ICONS 列表中的 SVG / 光栅图片）
  const iconUrl = useMemo(() => {
    if (icon && isUrlIcon(icon)) {
      return getIconUrl(icon);
    }
    return "";
  }, [icon]);

  // 计算尺寸样式
  const sizeStyle = useMemo(() => {
    const sizeValue = typeof size === "number" ? `${size}px` : size;
    return {
      width: sizeValue,
      height: sizeValue,
      fontSize: sizeValue,
      lineHeight: 1,
    };
  }, [size]);

  // 获取有效颜色：优先使用传入的有效 color，否则从元数据获取 defaultColor
  const effectiveColor = useMemo(() => {
    if (color && typeof color === "string" && color.trim() !== "") {
      return color;
    }
    if (icon) {
      const metadata = getIconMetadata(icon);
      if (metadata?.defaultColor && metadata.defaultColor !== "currentColor") {
        return metadata.defaultColor;
      }
    }
    return undefined;
  }, [color, icon]);

  // Bundled SVG icons are rendered from a data: URL, never injected as
  // markup (FE-ARCH-21), so an icon string can never run script. Monochrome
  // `currentColor` icons become a CSS mask filled with the text colour, which
  // keeps theme/hover colouring; other icons are plain images.
  if (iconSvg) {
    const rendering = svgIconRendering(iconSvg, effectiveColor);
    const boxClass = cn(
      "inline-flex items-center justify-center flex-shrink-0",
      className,
    );
    if (rendering.kind === "mask") {
      return (
        <span
          role="img"
          aria-label={name}
          title={name}
          className={boxClass}
          style={{
            ...sizeStyle,
            color: effectiveColor,
            backgroundColor: "currentColor",
            WebkitMaskImage: `url("${rendering.src}")`,
            maskImage: `url("${rendering.src}")`,
            WebkitMaskRepeat: "no-repeat",
            maskRepeat: "no-repeat",
            WebkitMaskPosition: "center",
            maskPosition: "center",
            WebkitMaskSize: "contain",
            maskSize: "contain",
          }}
        />
      );
    }
    return (
      <img
        src={rendering.src}
        alt={name}
        title={name}
        className={cn(boxClass, "object-contain")}
        style={{ width: sizeStyle.width, height: sizeStyle.height }}
        draggable={false}
      />
    );
  }

  // URL-based 图标（大型 SVG / 光栅图片）：以 <img> 渲染
  if (iconUrl) {
    return (
      <img
        src={iconUrl}
        alt={name}
        title={name}
        className={cn(
          "inline-flex items-center justify-center flex-shrink-0 object-contain",
          className,
        )}
        style={{ width: sizeStyle.width, height: sizeStyle.height }}
        loading="lazy"
      />
    );
  }

  // Fallback：显示首字母
  if (showFallback) {
    const initials = name
      .split(" ")
      .map((word) => word[0])
      .join("")
      .toUpperCase()
      .slice(0, 2);
    const fallbackFontSize =
      typeof size === "number" ? `${Math.max(size * 0.5, 12)}px` : "0.5em";
    return (
      <span
        className={cn(
          "inline-flex items-center justify-center flex-shrink-0 rounded-lg",
          "bg-muted text-muted-foreground font-semibold",
          className,
        )}
        title={name}
        style={sizeStyle}
      >
        <span
          style={{
            fontSize: fallbackFontSize,
          }}
        >
          {initials}
        </span>
      </span>
    );
  }

  return null;
};
