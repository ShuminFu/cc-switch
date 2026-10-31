import { describe, expect, it } from "vitest";
import {
  getApiKeyFromConfig,
  hasApiKeyField,
  setApiKeyInConfig,
} from "@/utils/providerConfigUtils";

const json = (value: unknown) => JSON.stringify(value);

describe("provider config API key helpers", () => {
  const env = {
    GEMINI_API_KEY: "gemini-key",
    CODEX_API_KEY: "codex-key",
    ANTHROPIC_AUTH_TOKEN: "anthropic-token",
  };

  it.each([
    ["gemini", "gemini-key"],
    ["codex", "codex-key"],
    ["claude", "anthropic-token"],
    ["claude-desktop", "anthropic-token"],
    [undefined, "anthropic-token"],
  ])("reads the %s key from env", (appType, expected) => {
    expect(getApiKeyFromConfig(json({ env }), appType)).toBe(expected);
  });

  it("prefers a top-level apiKey and ignores template placeholders", () => {
    expect(getApiKeyFromConfig(json({ apiKey: "top", env }), "gemini")).toBe(
      "top",
    );
    expect(getApiKeyFromConfig(json({ apiKey: "${KEY}", env }), "codex")).toBe(
      "codex-key",
    );
  });

  it("does not fall back to Anthropic variables for Gemini / Codex", () => {
    const anthropicOnly = json({ env: { ANTHROPIC_API_KEY: "a" } });
    expect(getApiKeyFromConfig(anthropicOnly, "gemini")).toBe("");
    expect(getApiKeyFromConfig(anthropicOnly, "codex")).toBe("");
    expect(getApiKeyFromConfig(anthropicOnly, "claude")).toBe("a");
    expect(hasApiKeyField(anthropicOnly, "gemini")).toBe(false);
    expect(hasApiKeyField(anthropicOnly, "codex")).toBe(false);
    expect(hasApiKeyField(anthropicOnly, "claude")).toBe(true);
  });

  it.each([
    ["gemini", { GEMINI_API_KEY: "" }],
    ["codex", { CODEX_API_KEY: "" }],
    ["claude", { ANTHROPIC_API_KEY: "" }],
    ["hermes", { ANTHROPIC_AUTH_TOKEN: "" }],
  ])("detects the %s key field", (appType, fields) => {
    expect(hasApiKeyField(json({ env: fields }), appType)).toBe(true);
    expect(hasApiKeyField(json({ env: {} }), appType)).toBe(false);
  });

  it.each([
    ["gemini", "GEMINI_API_KEY"],
    ["codex", "CODEX_API_KEY"],
    ["claude", "ANTHROPIC_AUTH_TOKEN"],
  ])("writes the %s key only when allowed", (appType, field) => {
    const empty = json({ env: {} });
    expect(setApiKeyInConfig(empty, "k", { appType })).toBe(empty);
    expect(
      JSON.parse(
        setApiKeyInConfig(empty, "k", { appType, createIfMissing: true }),
      ).env,
    ).toEqual({ [field]: "k" });
    expect(
      JSON.parse(
        setApiKeyInConfig(json({ env: { [field]: "old" } }), "k", { appType }),
      ).env,
    ).toEqual({ [field]: "k" });
  });

  it("ignores apiKeyField for Gemini / Codex and honours it for Claude", () => {
    const empty = json({});
    const options = { createIfMissing: true, apiKeyField: "ANTHROPIC_API_KEY" };
    expect(
      JSON.parse(
        setApiKeyInConfig(empty, "k", { ...options, appType: "gemini" }),
      ).env,
    ).toEqual({ GEMINI_API_KEY: "k" });
    expect(
      JSON.parse(
        setApiKeyInConfig(empty, "k", { ...options, appType: "codex" }),
      ).env,
    ).toEqual({ CODEX_API_KEY: "k" });
    expect(
      JSON.parse(
        setApiKeyInConfig(empty, "k", { ...options, appType: "claude" }),
      ).env,
    ).toEqual({ ANTHROPIC_API_KEY: "k" });
  });
});
