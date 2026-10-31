import { spawnSync } from "node:child_process";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { describe, expect, it } from "vitest";

const SCRIPT = path.resolve(__dirname, "../../scripts/check-repo.mjs");

/** Evaluate an expression against the script's exports in a separate Node process. */
function evalWith(expr: string): unknown {
  const code = `import * as m from ${JSON.stringify(pathToFileURL(SCRIPT).href)};
process.stdout.write(JSON.stringify(${expr}));`;
  const res = spawnSync(process.execPath, ["--input-type=module", "-e", code], {
    encoding: "utf8",
  });
  if (res.status !== 0) throw new Error(res.stderr);
  return JSON.parse(res.stdout);
}

describe("scripts/check-repo.mjs", () => {
  it("passes on the repository as committed", () => {
    const res = spawnSync(process.execPath, [SCRIPT], { encoding: "utf8" });
    expect(res.stderr).toBe("");
    expect(res.status).toBe(0);
  });

  it("matches labeler-style globs", () => {
    expect(
      evalWith(
        `[m.globToRegExp("src/**").test("src/a/b.ts"), m.globToRegExp("src-tauri/src/*_mcp.rs").test("src-tauri/src/claude_mcp.rs"), m.globToRegExp("src/*.ts").test("src/a/b.ts"), m.globToRegExp("*.md").test("README.md")]`,
      ),
    ).toEqual([true, true, false, true]);
  });

  it("recognises real release asset names and rejects stale ones", () => {
    expect(
      evalWith(
        `["Linux-x86_64.deb", "Windows-arm64.msi", "*.AppImage", "Linux-*.deb", "Linux.deb", "Windows-x64.msi"].map(m.isKnownAssetSuffix)`,
      ),
    ).toEqual([true, true, true, true, false, false]);
  });

  it("extracts asset names from docs in every placeholder style", () => {
    expect(
      evalWith(
        `m.mentionedAssetSuffixes("get CC-Switch-v{version}-Linux.deb or CC-Switch-v{版本号}-macOS.dmg, CC-Switch-v3.17.0-Windows.msi and CC-Switch-*.AppImage")`,
      ),
    ).toEqual(["Linux.deb", "macOS.dmg", "Windows.msi", "*.AppImage"]);
  });

  it("ignores headings inside code fences when comparing README structure", () => {
    expect(evalWith(`m.headingLevels("# A\\n## B\\n\`\`\`\\n# not a heading\\n\`\`\`\\n### C")`)).toEqual([
      1, 2, 3,
    ]);
  });
});
