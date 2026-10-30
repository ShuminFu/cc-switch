import path from "node:path";
import { spawnSync } from "node:child_process";
import { describe, expect, it } from "vitest";
import {
  checkIpcCommands,
  registeredCommands,
} from "../../scripts/check-ipc-orphans.mjs";

// CCS-118：已注册命令必须被前端引用、命名为 snake_case，前端也不能调用未注册命令
const ROOT = path.resolve(__dirname, "../..");
const SCRIPT = path.join(ROOT, "scripts/check-ipc-orphans.mjs");

const REGISTRY = [
  "macro_rules! for_each_command_domain {",
  "    ($callback:ident) => {",
  "        $callback! {",
  '            standard "provider" "Providers." [',
  "                commands::get_providers,",
  "                // a comment, commands::not_registered",
  "                commands::queryProviderUsage, commands::unused_command,",
  "            ],",
  '            privileged "privileged-x" "Writes [files]." [',
  "                crate::commands::open_external",
  "            ],",
  "        }",
  "    };",
  "}",
  "pub commands: &'static [&'static str],",
].join("\n");

describe("checkIpcOrphans", () => {
  it("parses the command registry domain lists", () => {
    expect(registeredCommands(REGISTRY)).toEqual([
      "get_providers",
      "queryProviderUsage",
      "unused_command",
      "open_external",
    ]);
  });

  it("reports orphan, unknown and non-snake_case commands", () => {
    const result = checkIpcCommands(REGISTRY, {
      "src/a.ts": [
        'await invoke<Provider[]>("get_providers", { app });',
        "return invoke(\n  'queryProviderUsage',\n  { providerId },\n);",
        'invoke("removed_command");',
        // 包装函数的 invoke<T>(cmd) 不能吞掉其后的调用
        "invoke<T>(cmd, args);",
        'invoke("also_removed");',
        'invoke<string>("get_providers");',
      ].join("\n"),
      "src/b.ts": 'const cmd = "open_external";',
    });
    expect(result).toEqual({
      total: 4,
      orphans: ["unused_command"],
      unknown: ["also_removed", "removed_command"],
      badNames: ["queryProviderUsage"],
    });
  });

  it("reports no orphan or misnamed commands in the real tree", () => {
    const r = spawnSync(process.execPath, [SCRIPT], {
      cwd: ROOT,
      encoding: "utf8",
    });
    expect(r.status, r.stderr).toBe(0);
    expect(r.stdout).toMatch(
      /IPC commands: \d+ registered, 0 orphans, 0 unknown, 0 non-snake_case/,
    );
  });
});
