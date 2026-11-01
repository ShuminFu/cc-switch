import { describe, expect, it } from "vitest";
import { parse as parseToml } from "smol-toml";
import {
  parseGrokBuildConfig,
  updateGrokBuildConfig,
  updateGrokBuildConfigPreservingFormat,
} from "@/utils/grokBuildConfig";

const HAND_EDITED = `# my grok setup
[models]
default = "grok-4.5"

[model."grok-4.5"]
# relay endpoint, keep this note
model = "grok-4.5"
base_url = "https://old.example.com/v1"
name = "Relay"
api_key = "secret"
api_backend = "responses"
context_window = 500000

[extra]
keep = true
`;

describe("updateGrokBuildConfigPreservingFormat (FE-PROV-31)", () => {
  it("keeps comments and order when the base URL changes", () => {
    const values = {
      ...parseGrokBuildConfig(HAND_EDITED),
      baseUrl: "https://new.example.com/v1",
    };
    const next = updateGrokBuildConfigPreservingFormat(HAND_EDITED, values);

    expect(next).toContain("# my grok setup");
    expect(next).toContain("# relay endpoint, keep this note");
    expect(next).toContain('base_url = "https://new.example.com/v1"');
    expect(next).not.toContain("old.example.com");
    // Semantically identical to the full rewrite.
    expect(parseToml(next)).toEqual(
      parseToml(updateGrokBuildConfig(HAND_EDITED, values)),
    );
  });

  it("falls back to the full rewrite when the profile changes", () => {
    const values = {
      ...parseGrokBuildConfig(HAND_EDITED),
      model: "grok-5",
    };
    expect(updateGrokBuildConfigPreservingFormat(HAND_EDITED, values)).toBe(
      updateGrokBuildConfig(HAND_EDITED, values),
    );
  });

  it("removes a cleared api_key line and appends new fields", () => {
    const values = {
      ...parseGrokBuildConfig(HAND_EDITED),
      apiKey: "",
      envKey: "XAI_API_KEY",
    };
    const next = updateGrokBuildConfigPreservingFormat(HAND_EDITED, values);
    expect(next).not.toContain("api_key =");
    expect(next).toContain('env_key = "XAI_API_KEY"');
    expect(next).toContain("# relay endpoint, keep this note");
  });
});
