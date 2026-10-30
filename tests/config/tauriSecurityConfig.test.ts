import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const repoRoot = path.resolve(__dirname, "../..");
const readJson = (rel: string) =>
  JSON.parse(fs.readFileSync(path.join(repoRoot, rel), "utf8"));

const parseCsp = (csp: string): Record<string, string[]> =>
  Object.fromEntries(
    csp
      .split(";")
      .map((d) => d.trim())
      .filter(Boolean)
      .map((d) => {
        const [name, ...sources] = d.split(/\s+/);
        return [name, sources];
      }),
  );

describe("Tauri security configuration", () => {
  const conf = readJson("src-tauri/tauri.conf.json");
  const capability = readJson("src-tauri/capabilities/default.json");
  const permissions: string[] = capability.permissions;

  it("grants process:allow-exit so frontend exit() calls are not denied (CI-39)", () => {
    expect(permissions).toContain("process:allow-exit");
    expect(permissions).toContain("process:allow-restart");
  });

  it("does not grant opener/dialog permissions the frontend never uses (CMD-03)", () => {
    expect(permissions).not.toContain("opener:default");
    expect(permissions).not.toContain("opener:allow-open-path");
    expect(permissions).not.toContain("opener:allow-reveal-item-in-dir");
    expect(permissions).not.toContain("dialog:default");
    // Needed: <a target="_blank"> links go through plugin:opener|open_url and
    // main.tsx shows an error via the dialog plugin's message().
    expect(permissions).toContain("opener:allow-open-url");
    expect(permissions).toContain("dialog:allow-message");
  });

  it("CSP connect-src and img-src contain no scheme-wide wildcard (CMD-03)", () => {
    const csp = parseCsp(conf.app.security.csp);
    for (const directive of ["connect-src", "img-src", "default-src"]) {
      const sources = csp[directive] ?? [];
      expect(sources, directive).not.toContain("http:");
      expect(sources, directive).not.toContain("https:");
      expect(sources, directive).not.toContain("*");
    }
    expect(csp["connect-src"]).toEqual(
      expect.arrayContaining(["'self'", "ipc:", "http://ipc.localhost"]),
    );
    // ModelsDevPickerDialog fetches pricing directly from the webview.
    expect(csp["connect-src"]).toContain("https://models.dev");
  });
});

describe("Vite env exposure (CI-40)", () => {
  it("does not expose TAURI_* variables (signing key) to the renderer bundle", () => {
    const viteConfig = fs.readFileSync(
      path.join(repoRoot, "vite.config.ts"),
      "utf8",
    );
    const match = viteConfig.match(/envPrefix:\s*\[([^\]]*)\]/);
    expect(match).not.toBeNull();
    const prefixes = [...match![1].matchAll(/["']([^"']+)["']/g)].map(
      (m) => m[1],
    );
    expect(prefixes.length).toBeGreaterThan(0);
    for (const prefix of prefixes) {
      expect("TAURI_SIGNING_PRIVATE_KEY".startsWith(prefix)).toBe(false);
      expect("TAURI_SIGNING_PRIVATE_KEY_PASSWORD".startsWith(prefix)).toBe(
        false,
      );
    }
  });
});
