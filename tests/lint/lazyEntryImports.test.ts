import path from "node:path";
import { readFileSync } from "node:fs";
import { describe, it, expect } from "vitest";

// 首屏包体积守卫（CCS-198）：这些重型模块只能通过 import() 按需加载，
// 一旦在应用外壳中被静态导入，会把 CodeMirror / prettier / recharts / 预设等拉回首屏包。
const ROOT = path.resolve(__dirname, "../..");

const SHELL_FILES = [
  "src/App.tsx",
  "src/components/app-shell/AppViewContent.tsx",
  "src/components/app-shell/AppHeader.tsx",
  "src/components/app-shell/HeaderViewActions.tsx",
  "src/components/app-shell/ProvidersHome.tsx",
  "src/components/providers/ProviderCard.tsx",
];

const LAZY_ONLY_MODULES = [
  "@/components/settings/SettingsPage",
  "@/components/prompts/PromptPanel",
  "@/components/mcp/UnifiedMcpPanel",
  "@/components/skills/SkillsPage",
  "@/components/skills/UnifiedSkillsPanel",
  "@/components/agents/AgentsPanel",
  "@/components/universal",
  "@/components/sessions/SessionManagerPage",
  "@/components/workspace/WorkspaceFilesPanel",
  "@/components/openclaw/EnvPanel",
  "@/components/openclaw/ToolsPanel",
  "@/components/openclaw/AgentsDefaultsPanel",
  "@/components/hermes/HermesMemoryPanel",
  "@/components/providers/AddProviderDialog",
  "@/components/providers/EditProviderDialog",
  "@/components/UsageScriptModal",
  "@/components/JsonEditor",
  "@/components/MarkdownEditor",
  "@/config/hermesProviderPresets",
];

/** 返回非 type-only 的静态 import 模块路径 */
const staticImports = (source: string): string[] =>
  Array.from(
    source.matchAll(/^import\s+(?!type\s)[^;]*?from\s+"([^"]+)";/gms),
    (m) => m[1],
  );

describe("entry bundle keeps heavy views lazy", () => {
  for (const file of SHELL_FILES) {
    it(`${file} does not statically import lazy-only modules`, () => {
      const imports = staticImports(
        readFileSync(path.join(ROOT, file), "utf8"),
      );
      const offenders = imports.filter((spec) =>
        LAZY_ONLY_MODULES.some(
          (mod) => spec === mod || spec.startsWith(`${mod}/`),
        ),
      );
      expect(offenders).toEqual([]);
    });
  }
});
