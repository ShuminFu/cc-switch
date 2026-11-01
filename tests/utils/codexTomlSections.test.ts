import { describe, expect, it } from "vitest";
import { parse as parseToml } from "smol-toml";
import {
  extractCodexBaseUrl,
  setCodexBaseUrl,
} from "@/utils/providerConfigUtils";

const URL = "https://new.example.com/v1";

// FE-ARCH-46: section detection must handle array tables, trailing comments
// and quoted / whitespace-padded keys.
describe("Codex TOML section detection", () => {
  it("inserts base_url before a following [[array.table]]", () => {
    const config = `model_provider = "x"

[model_providers.x]
name = "x"

[[profiles.y]]
base_url = "https://wrong.example.com"
`;
    const out = setCodexBaseUrl(config, URL);
    const parsed = parseToml(out) as any;
    expect(parsed.model_providers.x.base_url).toBe(URL);
    expect(parsed.profiles.y[0].base_url).toBe("https://wrong.example.com");
    expect(out.indexOf(`base_url = "${URL}"`)).toBeLessThan(
      out.indexOf("[[profiles.y]]"),
    );
  });

  it("recognises a header with a trailing comment", () => {
    const config = `model_provider = "x"

[model_providers.x] # main
name = "x"
base_url = "https://old.example.com"
`;
    expect(extractCodexBaseUrl(config)).toBe("https://old.example.com");
    const parsed = parseToml(setCodexBaseUrl(config, URL)) as any;
    expect(parsed.model_providers.x.base_url).toBe(URL);
  });

  it("matches a quoted provider id containing a dot", () => {
    const config = `model_provider = "a.b"

[model_providers."a.b"]
name = "a.b"
base_url = "https://old.example.com"
`;
    const out = setCodexBaseUrl(config, URL);
    const parsed = parseToml(out) as any;
    expect(parsed.model_providers["a.b"].base_url).toBe(URL);
    expect(out.match(/base_url/g)).toHaveLength(1);
  });

  it("matches whitespace-padded headers", () => {
    const config = `model_provider = "x"

[ model_providers . x ]
base_url = "https://old.example.com"
`;
    const parsed = parseToml(setCodexBaseUrl(config, URL)) as any;
    expect(parsed.model_providers.x.base_url).toBe(URL);
  });
});
