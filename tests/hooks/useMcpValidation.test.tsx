import { renderHook } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useMcpValidation } from "@/components/mcp/useMcpValidation";

const validateTomlMock = vi.hoisted(() => vi.fn());
const tomlToMcpServerMock = vi.hoisted(() => vi.fn());

vi.mock("@/utils/tomlUtils", () => ({
  validateToml: (...args: unknown[]) => validateTomlMock(...args),
  tomlToMcpServer: (...args: unknown[]) => tomlToMcpServerMock(...args),
}));

describe("useMcpValidation", () => {
  beforeEach(() => {
    validateTomlMock.mockReset();
    tomlToMcpServerMock.mockReset();
    validateTomlMock.mockReturnValue("");
  });

  const getHookResult = () =>
    renderHook(() => useMcpValidation()).result.current;

  describe("validateJson", () => {
    it("returns empty string for blank text", () => {
      const { validateJson } = getHookResult();
      expect(validateJson("   ")).toBe("");
    });

    it("returns error key when JSON parsing fails", () => {
      const { validateJson } = getHookResult();
      expect(validateJson("{ invalid")).toBe("Invalid JSON format");
    });

    it("returns error key when parsed value is not an object", () => {
      const { validateJson } = getHookResult();
      expect(validateJson('"string"')).toBe("Invalid JSON format");
      expect(validateJson("[]")).toBe("Invalid JSON format");
    });

    it("accepts valid object payload", () => {
      const { validateJson } = getHookResult();
      expect(validateJson('{"id":"demo"}')).toBe("");
    });
  });

  describe("formatTomlError", () => {
    it("maps mustBeObject and parseError to i18n key", () => {
      const { formatTomlError } = getHookResult();
      expect(formatTomlError("mustBeObject")).toBe("Invalid TOML format");
      expect(formatTomlError("parseError")).toBe("Invalid TOML format");
    });

    it("appends error message when details provided", () => {
      const { formatTomlError } = getHookResult();
      expect(formatTomlError("unknown")).toBe("Invalid TOML format: unknown");
    });
  });

  describe("validateTomlConfig", () => {
    it("propagates errors returned by validateToml", () => {
      validateTomlMock.mockReturnValue("parse-error-detail");
      const { validateTomlConfig } = getHookResult();
      expect(validateTomlConfig("foo")).toBe(
        "Invalid TOML format: parse-error-detail",
      );
      expect(tomlToMcpServerMock).not.toHaveBeenCalled();
    });

    it("returns command required when stdio server missing command", () => {
      tomlToMcpServerMock.mockReturnValue({
        type: "stdio",
        command: "   ",
      });
      const { validateTomlConfig } = getHookResult();
      expect(validateTomlConfig("foo")).toBe("Please enter command");
    });

    it("returns url required when http server missing url", () => {
      tomlToMcpServerMock.mockReturnValue({
        type: "http",
        url: "",
      });
      const { validateTomlConfig } = getHookResult();
      expect(validateTomlConfig("foo")).toBe("Please enter URL");
    });

    it("returns url required when sse server missing url", () => {
      tomlToMcpServerMock.mockReturnValue({
        type: "sse",
        url: "",
      });
      const { validateTomlConfig } = getHookResult();
      expect(validateTomlConfig("foo")).toBe("Please enter URL");
    });

    it("surface tomlToMcpServer errors via formatter", () => {
      tomlToMcpServerMock.mockImplementation(() => {
        throw new Error("normalize failed");
      });
      const { validateTomlConfig } = getHookResult();
      expect(validateTomlConfig("foo")).toBe(
        "Invalid TOML format: normalize failed",
      );
    });

    it("returns empty string when validation passes", () => {
      tomlToMcpServerMock.mockReturnValue({
        type: "stdio",
        command: "run.sh",
      });
      const { validateTomlConfig } = getHookResult();
      expect(validateTomlConfig("foo")).toBe("");
    });
  });

  describe("validateJsonConfig", () => {
    it("returns error when JSON invalid", () => {
      const { validateJsonConfig } = getHookResult();
      expect(validateJsonConfig("invalid")).toBe("Invalid JSON format");
    });

    it("rejects arrays of servers", () => {
      const { validateJsonConfig } = getHookResult();
      expect(validateJsonConfig('{"mcpServers": {}}')).toBe(
        "Please paste a single MCP server object (do not include top-level mcpServers)",
      );
    });

    it("requires command for stdio type", () => {
      const { validateJsonConfig } = getHookResult();
      expect(validateJsonConfig('{"type":"stdio"}')).toBe(
        "Please enter command",
      );
    });

    it("requires url for http type", () => {
      const { validateJsonConfig } = getHookResult();
      expect(validateJsonConfig('{"type":"http","url":""}')).toBe(
        "Please enter URL",
      );
    });

    it("requires url for sse type", () => {
      const { validateJsonConfig } = getHookResult();
      expect(validateJsonConfig('{"type":"sse","url":""}')).toBe(
        "Please enter URL",
      );
    });

    it("returns empty string when json config valid", () => {
      const { validateJsonConfig } = getHookResult();
      expect(
        validateJsonConfig(
          JSON.stringify({
            type: "stdio",
            command: "node",
            args: ["index.js"],
          }),
        ),
      ).toBe("");
    });
  });
});
