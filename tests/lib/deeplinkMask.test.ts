import { describe, expect, it } from "vitest";
import {
  maskConfigText,
  maskFieldValue,
  maskSecret,
  maskValue,
} from "@/lib/deeplink/mask";

describe("deeplink masking", () => {
  it("shows at most the first 4 characters of a sensitive value", () => {
    expect(maskValue("ANTHROPIC_AUTH_TOKEN", "sk-abcdefghijk")).toBe(
      `sk-a${"*".repeat(20)}`,
    );
  });

  it("hides sensitive values of 8 characters or fewer completely", () => {
    expect(maskValue("ANTHROPIC_AUTH_TOKEN", "abcdefgh")).toBe("****");
    expect(maskValue("ANTHROPIC_AUTH_TOKEN", "abcd")).toBe("****");
    expect(maskValue("OPENAI_API_KEY", "")).toBe("****");
  });

  it("matches sensitive key names case-insensitively", () => {
    expect(maskValue("gemini_api_key", "AIzaSyExample")).toBe(
      `AIza${"*".repeat(20)}`,
    );
    expect(maskValue("db_password", "hunter2!x")).toBe(`hunt${"*".repeat(20)}`);
  });

  it("leaves non-sensitive values unchanged", () => {
    expect(maskValue("ANTHROPIC_BASE_URL", "https://api.example.com")).toBe(
      "https://api.example.com",
    );
    expect(maskValue("ANTHROPIC_MODEL", "abc")).toBe("abc");
  });

  it("does not reveal the secret's length", () => {
    expect(maskSecret("abcdefghij")).toBe(
      maskSecret("abcdefghijklmnopqrstuvwxyz0123"),
    );
    expect(maskSecret(undefined)).toBe("****");
  });

  it("masks diff values by the leaf key of their dotted field path", () => {
    expect(
      maskFieldValue(
        "settingsConfig.env.ANTHROPIC_AUTH_TOKEN",
        "sk-abcdefghijk",
      ),
    ).toBe(`sk-a${"*".repeat(20)}`);
    expect(
      maskFieldValue("settingsConfig.env.ANTHROPIC_BASE_URL", "https://a.b"),
    ).toBe("https://a.b");
  });

  it("masks secret lines inside multi-line config text", () => {
    const toml =
      'model = "grok-4"\napi_key = "xai-abcdefghijkl"\nbase_url = "https://x.ai"';
    expect(maskConfigText(toml)).toBe(
      `model = "grok-4"\napi_key = "xai-${"*".repeat(20)}"\nbase_url = "https://x.ai"`,
    );
    expect(maskFieldValue("settingsConfig.config", toml)).toBe(
      maskConfigText(toml),
    );
  });
});
