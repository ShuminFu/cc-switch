import { describe, expect, it } from "vitest";
import {
  buildUnpricedModelsCsv,
  unpricedCsvFileName,
} from "@/components/usage/unpricedExport";
import { getPricingModelId } from "@/types/usage";

const stat = (model: string) => ({
  model,
  requestCount: 2,
  inputTokens: 10,
  outputTokens: 4,
  cacheReadTokens: 1,
  cacheCreationTokens: 0,
  firstSeenAt: 0,
  lastSeenAt: 60,
});

describe("buildUnpricedModelsCsv", () => {
  it("writes a header and one row per model", () => {
    const csv = buildUnpricedModelsCsv([stat("gpt-x"), stat("glm-y")]);
    expect(csv.split("\r\n")).toEqual([
      "model,request_count,input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,first_seen_at,last_seen_at",
      "gpt-x,2,10,4,1,0,1970-01-01T00:00:00.000Z,1970-01-01T00:01:00.000Z",
      "glm-y,2,10,4,1,0,1970-01-01T00:00:00.000Z,1970-01-01T00:01:00.000Z",
      "",
    ]);
  });

  it("quotes separators and neutralises formula-like model ids", () => {
    const csv = buildUnpricedModelsCsv([stat('a,"b"'), stat("=HYPERLINK(1)")]);
    const lines = csv.split("\r\n");
    expect(lines[1].startsWith('"a,""b""",2,')).toBe(true);
    expect(lines[2].startsWith("'=HYPERLINK(1),2,")).toBe(true);
  });

  it("names the file after the local date", () => {
    expect(unpricedCsvFileName(new Date(2026, 0, 5))).toBe(
      "unpriced-models-2026-01-05.csv",
    );
  });
});

describe("getPricingModelId", () => {
  it("prefers the stored pricing model unless it is a placeholder", () => {
    expect(getPricingModelId({ model: "echo", pricingModel: "real" })).toBe(
      "real",
    );
    expect(getPricingModelId({ model: "echo", pricingModel: "unknown" })).toBe(
      "echo",
    );
    expect(getPricingModelId({ model: "echo", pricingModel: "" })).toBe("echo");
    expect(getPricingModelId({ model: "echo" })).toBe("echo");
  });

  it("normalizes the id the way the backend price lookup does", () => {
    expect(
      getPricingModelId({
        model: "echo",
        pricingModel: "openrouter/Mystery-Chat:free",
      }),
    ).toBe("mystery-chat");
    expect(getPricingModelId({ model: "Vendor@V1[1m]" })).toBe("vendor-v1");
    // model 也是占位符时才退回 request_model
    expect(
      getPricingModelId({
        model: "unknown",
        pricingModel: "",
        requestModel: "mystery-alias",
      }),
    ).toBe("mystery-alias");
    expect(getPricingModelId({ model: "real", requestModel: "alias" })).toBe(
      "real",
    );
  });
});
