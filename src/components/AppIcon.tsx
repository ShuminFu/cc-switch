import type { AppId } from "@/lib/api/types";
import { APP_ICON_MAP, type AppIconSpec } from "@/config/appConfig";
import {
  ClaudeIcon,
  CodexIcon,
  GeminiIcon,
  OpenClawIcon,
} from "@/components/BrandIcons";
import { ProviderIcon } from "@/components/ProviderIcon";

const BRAND_ICONS = {
  claude: ClaudeIcon,
  codex: CodexIcon,
  gemini: GeminiIcon,
  openclaw: OpenClawIcon,
} as const;

export function renderAppIconSpec(spec: AppIconSpec, size = 14) {
  if (spec.kind === "brand") {
    const Brand = BRAND_ICONS[spec.brand];
    return <Brand size={size} />;
  }
  return (
    <ProviderIcon
      icon={spec.icon}
      name={spec.name}
      size={size}
      showFallback={false}
    />
  );
}

/** The glyph of an app, as described by `APP_ICON_MAP[app].icon`. */
export function AppIcon({ app, size = 14 }: { app: AppId; size?: number }) {
  return renderAppIconSpec(APP_ICON_MAP[app].icon, size);
}
