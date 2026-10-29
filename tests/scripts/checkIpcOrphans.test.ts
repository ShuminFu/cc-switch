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

const LIB = [
  "fn run() {",
  "    builder.invoke_handler(tauri::generate_handler![",
  "            commands::get_providers,",
  "            // a comment, commands::not_registered",
  "            commands::queryProviderUsage, commands::unused_command,",
  "            crate::commands::open_external",
  "        ]);",
  "}",
].join("\n");

describe("checkIpcOrphans", () => {
  it("parses the generate_handler! list", () => {
    expect(registeredCommands(LIB)).toEqual([
      "get_providers",
      "queryProviderUsage",
      "unused_command",
      "open_external",
    ]);
  });

  it("reports orphan, unknown and non-snake_case commands", () => {
    const result = checkIpcCommands(LIB, {
      "src/a.ts": [
        'await invoke<Provider[]>("get_providers", { app });',
        "return invoke(\n  'queryProviderUsage',\n  { providerId },\n);",
        'invoke("removed_command");',
      ].join("\n"),
      "src/b.ts": 'const cmd = "open_external";',
    });
    expect(result).toEqual({
      total: 4,
      orphans: ["unused_command"],
      unknown: ["removed_command"],
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
