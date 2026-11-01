import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import {
  ENGLISH_ONLY_PAGES,
  HELP_TOPICS,
  MANUAL_BASE_URL,
  helpUrl,
  manualLanguage,
} from "@/lib/helpLinks";

const MANUAL_DIR = path.resolve(__dirname, "../../docs/user-manual");

describe("help links (DOCS-16)", () => {
  it("every topic points at a manual page that exists in each language", () => {
    for (const [topic, page] of Object.entries(HELP_TOPICS)) {
      const languages = ENGLISH_ONLY_PAGES.has(page)
        ? ["en"]
        : ["en", "zh", "ja"];
      for (const lang of languages) {
        expect(
          fs.existsSync(path.join(MANUAL_DIR, lang, page)),
          `${topic}: docs/user-manual/${lang}/${page}`,
        ).toBe(true);
      }
    }
  });

  it("maps UI languages to manual languages", () => {
    expect(manualLanguage("ja")).toBe("ja");
    expect(manualLanguage("zh")).toBe("zh");
    expect(manualLanguage("zh-TW")).toBe("zh");
    expect(manualLanguage("en")).toBe("en");
    expect(manualLanguage("de")).toBe("en");
    expect(manualLanguage(undefined)).toBe("en");
  });

  it("builds the URL in the active language with English fallback", () => {
    expect(helpUrl("proxy.failover", "zh-TW")).toBe(
      `${MANUAL_BASE_URL}/zh/4-proxy/4.3-failover.md`,
    );
    expect(helpUrl("proxy.failover", "ja", "Queue order")).toBe(
      `${MANUAL_BASE_URL}/ja/4-proxy/4.3-failover.md#Queue%20order`,
    );
    // Only the English manual has the command-line page.
    expect(helpUrl("faq.command-line", "ja")).toBe(
      `${MANUAL_BASE_URL}/en/5-faq/5.5-command-line.md`,
    );
  });
});
