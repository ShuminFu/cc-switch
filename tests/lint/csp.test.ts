import path from "node:path";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { describe, it, expect } from "vitest";

// CSP 收紧守卫（CCS-077）：webview 只允许直连 models.dev，图片不允许明文 http:。
// 新增前端直连 fetch 时必须同步更新 connect-src，否则会在运行时被 CSP 拦截。
const ROOT = path.resolve(__dirname, "../..");

function readCsp(): Map<string, string[]> {
  const conf = JSON.parse(
    readFileSync(path.join(ROOT, "src-tauri/tauri.conf.json"), "utf8"),
  );
  const csp: string = conf.app.security.csp;
  const directives = new Map<string, string[]>();
  for (const part of csp.split(";")) {
    const [name, ...sources] = part.trim().split(/\s+/);
    if (name) directives.set(name, sources);
  }
  return directives;
}

function listSourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = path.join(dir, entry);
    if (statSync(full).isDirectory()) {
      out.push(...listSourceFiles(full));
    } else if (/\.(ts|tsx)$/.test(entry) && !/\.test\.tsx?$/.test(entry)) {
      out.push(full);
    }
  }
  return out;
}

// 直连网络的前端文件 -> 其访问的 origin
const ALLOWED_DIRECT_FETCHES: Record<string, string> = {
  "src/components/usage/ModelsDevPickerDialog.tsx": "https://models.dev",
};

describe("tauri CSP", () => {
  it("connect-src only allows IPC and models.dev", () => {
    expect(readCsp().get("connect-src")).toEqual([
      "'self'",
      "ipc:",
      "http://ipc.localhost",
      "https://models.dev",
    ]);
  });

  it("img-src does not allow plain http images", () => {
    const img = readCsp().get("img-src") ?? [];
    expect(img).not.toContain("http:");
    expect(img).toEqual(["'self'", "data:", "https:"]);
  });

  it("no directive allows a bare http: scheme", () => {
    for (const [name, sources] of readCsp()) {
      expect(sources, name).not.toContain("http:");
    }
  });

  it("only allow-listed frontend files open direct network connections", () => {
    const pattern =
      /(?<![\w.$])fetch\s*\(|\bXMLHttpRequest\b|\bnew\s+WebSocket\b|\bnew\s+EventSource\b|\bsendBeacon\s*\(/;
    const offenders = listSourceFiles(path.join(ROOT, "src"))
      .map((file) => path.relative(ROOT, file).split(path.sep).join("/"))
      .filter((rel) => pattern.test(readFileSync(path.join(ROOT, rel), "utf8")))
      .sort();
    expect(offenders).toEqual(Object.keys(ALLOWED_DIRECT_FETCHES).sort());

    const connectSrc = readCsp().get("connect-src") ?? [];
    for (const [file, origin] of Object.entries(ALLOWED_DIRECT_FETCHES)) {
      expect(readFileSync(path.join(ROOT, file), "utf8")).toContain(origin);
      expect(connectSrc, file).toContain(origin);
    }
  });
});
