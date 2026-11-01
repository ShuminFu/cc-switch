import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import {
  manualLanguage,
  manualRestoreUrl,
} from "@/components/common/manualLinks";

const ROOT = path.resolve(__dirname, "../..");

/** GitHub's heading slug for a simple heading (lowercase, spaces → dashes). */
const slug = (heading: string) =>
  heading
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s-]/gu, "")
    .replace(/\s+/g, "-");

describe("manual restore link (DOCS-07)", () => {
  it("maps UI languages to manual folders", () => {
    expect(manualLanguage("en")).toBe("en");
    expect(manualLanguage("zh-TW")).toBe("zh");
    expect(manualLanguage("ja")).toBe("ja");
    expect(manualLanguage(undefined)).toBe("en");
  });

  it.each(["en", "zh", "ja"])(
    "points at an existing restore heading in the %s manual",
    (lang) => {
      const url = new URL(manualRestoreUrl(lang));
      const rel = url.pathname.split("/blob/main/")[1];
      const file = path.join(ROOT, rel);
      expect(fs.existsSync(file), rel).toBe(true);
      const anchor = decodeURIComponent(url.hash.slice(1));
      const headings = fs
        .readFileSync(file, "utf8")
        .split("\n")
        .filter((l) => l.startsWith("#"))
        .map((l) => slug(l.replace(/^#+/, "")));
      expect(headings).toContain(anchor);
    },
  );
});
