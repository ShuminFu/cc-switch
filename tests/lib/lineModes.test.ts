import { describe, expect, it } from "vitest";
import {
  tokenizeLine,
  tomlDiagnostics,
  type LineMode,
} from "@/lib/codemirror/lineModes";

const kinds = (mode: LineMode, line: string) =>
  tokenizeLine(mode, line).map((t) => [t.kind, line.slice(t.from, t.to)]);

describe("TOML line tokenizer (FE-PROV-16)", () => {
  it("highlights table headers and comments", () => {
    expect(kinds("toml", "[model_providers.x]")).toEqual([
      ["header", "[model_providers.x]"],
    ]);
    expect(kinds("toml", "[[profiles]] # list")).toEqual([
      ["header", "[[profiles]]"],
      ["comment", "# list"],
    ]);
    expect(kinds("toml", "  # a comment")).toEqual([
      ["comment", "# a comment"],
    ]);
  });

  it("splits key, string, atom and trailing comment", () => {
    expect(kinds("toml", 'base_url = "https://x/#frag" # note')).toEqual([
      ["key", "base_url"],
      ["string", '"https://x/#frag"'],
      ["comment", "# note"],
    ]);
    expect(kinds("toml", "requires_openai_auth = true")).toEqual([
      ["key", "requires_openai_auth"],
      ["atom", "true"],
    ]);
    expect(kinds("toml", 'a.b."c d" = 12')).toEqual([
      ["key", 'a.b."c d"'],
      ["atom", "12"],
    ]);
  });
});

describe("dotenv line tokenizer", () => {
  it("marks key and value; comments win", () => {
    expect(kinds("dotenv", "GEMINI_API_KEY=sk-1")).toEqual([
      ["key", "GEMINI_API_KEY"],
      ["string", "sk-1"],
    ]);
    expect(kinds("dotenv", "export FOO = bar")).toEqual([
      ["key", "FOO"],
      ["string", "bar"],
    ]);
    expect(kinds("dotenv", "# GEMINI_MODEL=x")).toEqual([
      ["comment", "# GEMINI_MODEL=x"],
    ]);
  });
});

describe("tomlDiagnostics", () => {
  it("is empty for valid or blank text", () => {
    expect(tomlDiagnostics("")).toEqual([]);
    expect(tomlDiagnostics('model = "x"\n[a]\nb = 1\n')).toEqual([]);
  });

  it("underlines the line the parser reports", () => {
    const text = 'model = "x"\n[broken\nb = 1';
    const [diag] = tomlDiagnostics(text);
    expect(diag.severity).toBe("error");
    const secondLineStart = text.indexOf("[broken");
    expect(diag.from).toBeGreaterThanOrEqual(secondLineStart);
    expect(diag.to).toBeLessThanOrEqual(secondLineStart + "[broken".length);
    expect(diag.message).not.toContain("\n");
  });
});
