import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import {
  END,
  README_BY_LOCALE,
  START,
  extract,
  outOfSync,
  parseTable,
  readBlock,
  renderTable,
  writeBlock,
} from "../../scripts/sync-readme.mjs";

const ROOT = path.resolve(__dirname, "../..");

const row = (href: string, logo: string, alt: string, blurb: string) =>
  `<tr>\n<td width="180"><a href="${href}"><img src="${logo}" alt="${alt}" width="150"></a></td>\n<td>${blurb}</td>\n</tr>`;

const readme = (rows: string[]) =>
  `# Title\n\n${START}\n<table>\n${rows.join("\n\n")}\n\n</table>\n${END}\n\n## Next\n`;

describe("scripts/sync-readme.mjs (CI-33)", () => {
  it("the committed READMEs match assets/partners/sponsors.json", () => {
    expect(outOfSync(ROOT)).toEqual([]);
  });

  it("round-trips a table byte for byte, including multi-line blurbs", () => {
    const md = readme([
      row("https://a.example", "logos/a.png", "A", "Thanks to A!"),
      row("https://b.example", "logos/b.png", "B", "Line one\nline two"),
    ]);
    const data = extract({ en: md });
    expect(renderTable(data, "en")).toBe(readBlock(md));
    expect(parseTable(readBlock(md) ?? "")).toHaveLength(2);
  });

  it("keeps shared links once and records per-locale overrides", () => {
    const en = readme([row("https://a.example", "a-en.png", "A", "Hi")]);
    const zh = readme([row("https://a.example/zh", "a-zh.png", "A", "你好")]);
    const data = extract({ en, zh });
    expect(data.sponsors[0]).toEqual({
      alt: "A",
      href: "https://a.example",
      logo: "a-en.png",
      blurb: { en: "Hi", zh: "你好" },
      overrides: { zh: { href: "https://a.example/zh", logo: "a-zh.png" } },
    });
    expect(renderTable(data, "zh")).toBe(readBlock(zh));
  });

  it("adding a sponsor is one JSON edit, and the check catches hand edits", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "sync-readme-"));
    try {
      const base = readme([row("https://a.example", "a.png", "A", "Hi")]);
      for (const file of Object.values(README_BY_LOCALE)) {
        fs.writeFileSync(path.join(dir, file), base);
      }
      const data = extract(
        Object.fromEntries(
          Object.keys(README_BY_LOCALE).map((locale) => [locale, base]),
        ),
      );
      fs.mkdirSync(path.join(dir, "assets/partners"), { recursive: true });
      const write = () =>
        fs.writeFileSync(
          path.join(dir, "assets/partners/sponsors.json"),
          JSON.stringify(data),
        );
      write();
      expect(outOfSync(dir)).toEqual([]);

      data.sponsors.push({
        alt: "New",
        href: "https://new.example",
        logo: "new.png",
        blurb: { en: "N", zh: "N", ja: "N", de: "N" },
      });
      write();
      expect(outOfSync(dir)).toEqual(Object.values(README_BY_LOCALE));

      const regenerated = writeBlock(base, renderTable(data, "en"));
      expect(regenerated).toContain('alt="New"');
      expect(regenerated.endsWith(`${END}\n\n## Next\n`)).toBe(true);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("refuses a locale whose blurb is missing", () => {
    expect(() =>
      renderTable(
        {
          sponsors: [
            { alt: "A", href: "h", logo: "l", blurb: { en: "only english" } },
          ],
        },
        "ja",
      ),
    ).toThrow(/no ja blurb/);
  });
});
