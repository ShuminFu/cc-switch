import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { mergeEnvIntoText } from "@/utils/providerEnvText";
import { useGeminiConfigState } from "@/components/providers/forms/hooks/useGeminiConfigState";

describe("mergeEnvIntoText (FE-PROV-48)", () => {
  it("updates keys in place and keeps comments, blank lines and order", () => {
    expect(
      mergeEnvIntoText("# note\nGEMINI_API_KEY=a\n\nFOO=1", {
        GEMINI_API_KEY: "b",
        FOO: "1",
      }),
    ).toBe("# note\nGEMINI_API_KEY=b\n\nFOO=1");
  });

  it("appends new keys and removes keys that are gone", () => {
    expect(
      mergeEnvIntoText("# keep\nOLD=1\nFOO=2", { FOO: "2", NEW: "3" }),
    ).toBe("# keep\nFOO=2\nNEW=3");
  });

  it("omits empty well-known keys like the editor did", () => {
    expect(mergeEnvIntoText("GEMINI_MODEL=x", { GEMINI_MODEL: "" })).toBe("");
  });
});

describe("useGeminiConfigState keeps annotations", () => {
  it("typing an API key does not drop comments", () => {
    const { result } = renderHook(() => useGeminiConfigState({}));
    act(() => {
      result.current.handleGeminiEnvChange(
        "# prod key\nGEMINI_API_KEY=a\n\nFOO=1",
      );
    });
    act(() => {
      result.current.handleGeminiApiKeyChange("b");
    });
    expect(result.current.geminiEnv).toBe(
      "# prod key\nGEMINI_API_KEY=b\n\nFOO=1",
    );
  });
});
