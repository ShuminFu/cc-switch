import path from "node:path";
import { readFileSync } from "node:fs";
import { describe, it, expect } from "vitest";
import {
  METAINFO,
  MAX_RELEASES,
  applyReleases,
  changelogReleases,
  renderReleases,
  syncMetainfo,
} from "../../scripts/sync-metainfo-releases.mjs";

// Flatpak 清单与 metainfo 需保持可维护（CCS-044）：<releases> 由 CHANGELOG.md 生成并在
// CI 中校验；清单使用受支持的 GNOME 运行时并以 commit 固定 git 源；release.yml 不再
// 安装从未使用的 flatpak 工具。
const ROOT = path.resolve(__dirname, "../..");
const read = (rel: string) => readFileSync(path.join(ROOT, rel), "utf8");

const CHANGELOG = [
  "# Changelog",
  "",
  "## [Unreleased]",
  "",
  "## [1.2.0] - 2026-02-01",
  "",
  "- y",
  "",
  "## [1.1.0] - 2026-01-01",
  "",
  "- x",
  "",
].join("\n");

const XML = [
  '<?xml version="1.0" encoding="UTF-8"?>',
  '<component type="desktop-application">',
  "  <id>x</id>",
  "</component>",
  "",
].join("\n");

describe("sync-metainfo-releases", () => {
  it("reads dated CHANGELOG entries and skips Unreleased", () => {
    expect(changelogReleases(CHANGELOG)).toEqual([
      { version: "1.2.0", date: "2026-02-01" },
      { version: "1.1.0", date: "2026-01-01" },
    ]);
  });

  it("inserts a <releases> block and replaces it on the next run", () => {
    const once = syncMetainfo({
      xml: XML,
      version: "1.2.0",
      changelog: CHANGELOG,
    });
    expect(once).toContain('<release version="1.2.0" date="2026-02-01">');
    expect(once.indexOf("<releases>")).toBeLessThan(
      once.indexOf("</component>"),
    );
    // 幂等：再次生成不变；新版本条目替换旧块而不是追加
    expect(
      syncMetainfo({ xml: once, version: "1.2.0", changelog: CHANGELOG }),
    ).toBe(once);
    const next = syncMetainfo({
      xml: once,
      version: "1.3.0",
      changelog: CHANGELOG.replace(
        "## [1.2.0]",
        "## [1.3.0] - 2026-03-01\n\n## [1.2.0]",
      ),
    });
    expect(next.match(/<releases>/g)).toHaveLength(1);
    expect(next).toContain('<release version="1.3.0" date="2026-03-01">');
  });

  it("caps the number of listed releases", () => {
    const many = Array.from(
      { length: MAX_RELEASES + 5 },
      (_, i) => `## [1.0.${MAX_RELEASES + 5 - i}] - 2026-01-01`,
    ).join("\n\n");
    const block = renderReleases(
      changelogReleases(many).slice(0, MAX_RELEASES),
    );
    expect(applyReleases(XML, block).match(/<release /g)).toHaveLength(
      MAX_RELEASES,
    );
  });

  it("fails when the package version has no dated entry, except prereleases", () => {
    expect(() =>
      syncMetainfo({ xml: XML, version: "9.9.9", changelog: CHANGELOG }),
    ).toThrow(/9\.9\.9/);
    expect(() =>
      syncMetainfo({ xml: XML, version: "1.3.0-rc.1", changelog: CHANGELOG }),
    ).not.toThrow();
  });

  it("the committed metainfo is up to date with CHANGELOG.md", () => {
    const xml = read(METAINFO);
    const version = JSON.parse(read("package.json")).version as string;
    expect(
      syncMetainfo({ xml, version, changelog: read("CHANGELOG.md") }),
    ).toBe(xml);
    expect(xml).toMatch(/<content_rating type="oars-1\.1"/);
    expect(xml).toMatch(/<developer id="[^"]+">/);
  });

  it("CI checks the metainfo releases", () => {
    expect(read(".github/workflows/ci.yml")).toMatch(
      /run:\s*node scripts\/sync-metainfo-releases\.mjs --check/,
    );
  });
});

describe("flatpak manifest", () => {
  const manifest = read("flatpak/com.ccswitch.desktop.yml");

  it("targets a supported GNOME runtime that the README matches", () => {
    const m = manifest.match(/^runtime-version:\s*'(\d+)'/m);
    expect(m).not.toBeNull();
    const runtime = Number(m![1]);
    expect(runtime).toBeGreaterThanOrEqual(50);
    const readme = read("flatpak/README.md");
    for (const ref of readme.matchAll(
      /org\.gnome\.(?:Platform|Sdk)\/\/(\d+)/g,
    )) {
      expect(Number(ref[1])).toBe(runtime);
    }
  });

  it("pins every git source to a commit", () => {
    const sources = manifest
      .split(/^\s*- type: /m)
      .filter((s) => s.startsWith("git"));
    expect(sources.length).toBeGreaterThan(0);
    for (const src of sources) {
      expect(src).toMatch(/^\s*commit:\s*[0-9a-f]{40}\s*$/m);
    }
  });

  it("release.yml does not install flatpak tooling it never uses", () => {
    expect(read(".github/workflows/release.yml")).not.toMatch(/flatpak/i);
  });
});
