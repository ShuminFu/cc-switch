import path from "node:path";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { describe, it, expect } from "vitest";

// asset 协议守卫（CCS-078）：前端不使用 asset://，因此 assetProtocol 与
// tauri 的 protocol-asset feature 一并移除（tauri-build 要求两者保持一致）。
const ROOT = path.resolve(__dirname, "../..");

function listSourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = path.join(dir, entry);
    if (statSync(full).isDirectory()) {
      out.push(...listSourceFiles(full));
    } else if (/\.(ts|tsx|js|jsx|css|html|json|svg)$/.test(entry)) {
      out.push(full);
    }
  }
  return out;
}

describe("tauri asset protocol", () => {
  it("is not enabled in any tauri config", () => {
    for (const name of ["tauri.conf.json", "tauri.windows.conf.json"]) {
      const conf = JSON.parse(
        readFileSync(path.join(ROOT, "src-tauri", name), "utf8"),
      );
      expect(conf.app?.security?.assetProtocol, name).toBeUndefined();
    }
  });

  it("tauri crate does not enable the protocol-asset feature", () => {
    const cargo = readFileSync(path.join(ROOT, "src-tauri/Cargo.toml"), "utf8");
    const tauriDep = cargo.split("\n").find((line) => /^tauri\s*=/.test(line));
    expect(tauriDep).toBeDefined();
    expect(tauriDep).not.toContain("protocol-asset");
  });

  it("frontend does not load files through the asset protocol", () => {
    const offenders = listSourceFiles(path.join(ROOT, "src"))
      .filter((file) =>
        /convertFileSrc|asset:\/\//.test(readFileSync(file, "utf8")),
      )
      .map((file) => path.relative(ROOT, file));
    expect(offenders).toEqual([]);
  });
});
