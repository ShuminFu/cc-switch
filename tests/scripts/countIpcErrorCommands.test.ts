import path from "node:path";
import { spawnSync } from "node:child_process";
import { describe, expect, it } from "vitest";
import {
  countIpcErrorCommands,
  scanCommands,
} from "../../scripts/count-ipc-error-commands.mjs";

// CCS-114 / CCS-181：全部 Tauri 命令返回 IpcError
const ROOT = path.resolve(__dirname, "../..");
const SCRIPT = path.join(ROOT, "scripts/count-ipc-error-commands.mjs");

describe("countIpcErrorCommands", () => {
  it("classifies command return types", () => {
    const source = [
      "#[tauri::command]",
      "pub async fn a(state: State<'_, AppState>) -> Result<Vec<(String, u8)>, IpcError> {",
      "    Ok(vec![])",
      "}",
      '#[tauri::command(rename_all = "camelCase")]',
      "pub fn b() -> Result<HashMap<String, String>, String> { todo!() }",
      "#[tauri::command]",
      "pub fn c() -> bool { true }",
      "fn not_a_command() -> Result<(), IpcError> { Ok(()) }",
    ].join("\n");
    expect(scanCommands(source)).toEqual([
      { name: "a", ipcError: true },
      { name: "b", ipcError: false },
      { name: "c", ipcError: false },
    ]);

    const result = countIpcErrorCommands({ "x.rs": source, "y.rs": "" });
    expect(result.total).toBe(3);
    expect(result.ipcError).toBe(1);
    expect(result.perFile).toEqual([{ file: "x.rs", total: 3, ipcError: 1 }]);
  });

  it("reports every real command returning IpcError", () => {
    const r = spawnSync(process.execPath, [SCRIPT, "--min", "100"], {
      cwd: ROOT,
      encoding: "utf8",
    });
    expect(r.status, r.stderr).toBe(0);
    expect(r.stdout).toMatch(/IpcError commands: \d+\/\d+ \(\d+\.\d%\)/);
  });
});
