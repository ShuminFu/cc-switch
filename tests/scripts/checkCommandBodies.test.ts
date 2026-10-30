import path from "node:path";
import { spawnSync } from "node:child_process";
import { describe, expect, it } from "vitest";
import {
  checkCommandBodies,
  scanCommandBodies,
  stripLiterals,
} from "../../scripts/check-command-bodies.mjs";

// CCS-181：命令只做参数解析与错误转换，函数体不超过 40 行且不直接查询数据库
const ROOT = path.resolve(__dirname, "../..");
const SCRIPT = path.join(ROOT, "scripts/check-command-bodies.mjs");

const body = (n: number) =>
  Array.from({ length: n }, (_, i) => `    let _v${i} = ${i};`).join("\n");

describe("checkCommandBodies", () => {
  it("measures command bodies and ignores braces inside literals", () => {
    const source = [
      "#[tauri::command]",
      "pub async fn thin(state: State<'_, AppState>) -> Result<(), IpcError> {",
      '    let s = "{ not a brace }"; // }}}',
      "    let c = '{';",
      '    let r = r#"}"#;',
      "    Service::run(&state.db, state.db.clone()).map_err(IpcError::from)",
      "}",
      "#[tauri::command]",
      "pub fn one_liner() -> Result<bool, IpcError> { Ok(true) }",
      "fn helper() { state.db.get_x(); }",
    ].join("\n");
    expect(scanCommandBodies(source)).toEqual([
      { name: "thin", line: 1, lines: 4, dbAccess: false },
      { name: "one_liner", line: 8, lines: 0, dbAccess: false },
    ]);
    expect(stripLiterals('a("x{y}") // {')).toBe('a("    ")     ');
  });

  it("flags long bodies and direct database queries", () => {
    const source = [
      "#[tauri::command]",
      "pub fn long() -> Result<(), IpcError> {",
      body(41),
      "    Ok(())",
      "}",
      "#[tauri::command]",
      "pub fn at_limit() -> Result<(), IpcError> {",
      body(39),
      "    Ok(())",
      "}",
      "#[tauri::command]",
      "pub fn queries(state: State<'_, AppState>) -> Result<(), IpcError> {",
      "    state",
      "        .db",
      "        .get_providers()?;",
      "    Ok(())",
      "}",
      "#[tauri::command]",
      "pub fn locks(state: State<'_, AppState>) -> Result<(), IpcError> {",
      "    let db = state.db.clone();",
      "    let conn = lock_conn!(db.conn);",
      "    Ok(())",
      "}",
    ].join("\n");
    const { total, violations } = checkCommandBodies({ "x.rs": source });
    expect(total).toBe(4);
    expect(violations).toEqual([
      {
        file: "x.rs",
        line: 1,
        name: "long",
        reason: "body is 42 lines (max 40)",
      },
      {
        file: "x.rs",
        line: 89,
        name: "queries",
        reason: "queries the database directly (move it into a service)",
      },
      {
        file: "x.rs",
        line: 96,
        name: "locks",
        reason: "queries the database directly (move it into a service)",
      },
    ]);
  });

  it("passes on the real command tree", () => {
    const r = spawnSync(process.execPath, [SCRIPT], {
      cwd: ROOT,
      encoding: "utf8",
    });
    expect(r.status, r.stderr).toBe(0);
    expect(r.stdout).toMatch(/\d+ commands are thin adapters/);
  });
});
