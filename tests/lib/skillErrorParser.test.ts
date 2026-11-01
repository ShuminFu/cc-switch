import { describe, it, expect } from "vitest";
import type { TFunction } from "i18next";
import { formatSkillError } from "@/lib/errors/skillErrorParser";

// Minimal t(): returns the interpolated defaultValue, else the key.
const t = ((key: string, options?: Record<string, string>) => {
  const template = options?.defaultValue ?? key;
  return template.replace(
    /\{\{(\w+)\}\}/g,
    (_, name: string) => options?.[name] ?? "",
  );
}) as unknown as TFunction;

const err = (
  code: string,
  context: Record<string, string>,
  suggestion?: string,
) => JSON.stringify({ code, context, suggestion });

describe("formatSkillError archive and directory codes", () => {
  it.each([
    [
      err("ARCHIVE_UNSAFE_ENTRY", { entry: "../escape.txt" }),
      "The archive contains an unsafe entry and was rejected: ../escape.txt",
    ],
    [
      err("ARCHIVE_TOO_MANY_ENTRIES", { entries: "20001", limit: "10000" }),
      "The archive has too many entries (20001, limit 10000).",
    ],
    [
      err("ARCHIVE_TOO_LARGE", { limit_bytes: String(200 * 1024 * 1024) }),
      "The archive is too large (limit 200 MB).",
    ],
    [
      err("INVALID_SKILL_DIRECTORY", { directory: "../x" }),
      "Invalid or unsafe skill directory: ../x",
    ],
    [
      err("INVALID_SKILL_DIRECTORY", { zip: "/tmp/a.zip" }),
      "The archive does not contain a valid skill directory.",
    ],
  ])("maps %s", (raw, expected) => {
    expect(formatSkillError(raw, t).description).toBe(expected);
  });

  it("no longer falls back to the unknown-error key for these codes", () => {
    const { description } = formatSkillError(
      err("ARCHIVE_UNSAFE_ENTRY", { entry: "/etc/passwd" }, "checkZipContent"),
      t,
    );
    expect(description).not.toContain("skills.error.unknownError");
    expect(description).toContain("skills.error.suggestion.checkZipContent");
  });
});
