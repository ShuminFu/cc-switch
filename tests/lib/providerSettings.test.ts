import { describe, expect, it } from "vitest";
import {
  narrowSettingsConfig,
  settingsConfigFor,
  type TypedSettingsConfig,
} from "@/types/providerSettings";

describe("narrowSettingsConfig (FE-ARCH-28)", () => {
  it("keeps well-typed Codex fields and unknown keys", () => {
    const codex = settingsConfigFor("codex", {
      auth: { OPENAI_API_KEY: "sk" },
      config: 'model = "gpt"',
      extra: 1,
    });
    expect(codex.config).toBe('model = "gpt"');
    expect(codex.auth).toEqual({ OPENAI_API_KEY: "sk" });
    expect(codex.extra).toBe(1);
  });

  it("drops typed fields of the wrong type instead of trusting them", () => {
    const codex = settingsConfigFor("codex", { auth: "nope", config: 42 });
    expect(codex.auth).toBeUndefined();
    expect(codex.config).toBeUndefined();
    const gemini = settingsConfigFor("gemini", { env: [], config: null });
    expect(gemini).toEqual({});
  });

  it("turns non-objects into an empty config", () => {
    expect(settingsConfigFor("claude", null)).toEqual({});
    expect(settingsConfigFor("hermes", "x")).toEqual({});
  });

  it("produces a union that narrows on app", () => {
    const typed: TypedSettingsConfig = narrowSettingsConfig("grokbuild", {
      config: "[x]",
    });
    if (typed.app === "grokbuild") {
      expect(typed.config.config).toBe("[x]");
    } else {
      throw new Error("expected grokbuild");
    }
  });
});
