import { describe, expect, it } from "vitest";
import type { TFunction } from "i18next";
import { IPC_ERROR_CODES, IpcError, type IpcErrorCode } from "@/lib/api/ipc";
import { translateMcpBackendError } from "@/utils/errorUtils";

const t = ((key: string) => key) as unknown as TFunction;

const ipcError = (code: IpcErrorCode, message = `${code} message`) =>
  new IpcError({ code, message, details: null, i18nKey: null });

const MAPPED: Partial<Record<IpcErrorCode, string>> = {
  MCP_VALIDATION: "mcp.error.validationFailed",
  JSON: "mcp.error.jsonInvalid",
  TOML: "mcp.error.tomlInvalid",
};

describe("translateMcpBackendError switches on IpcError.code", () => {
  it.each(IPC_ERROR_CODES.map((code) => [code, MAPPED[code] ?? ""]))(
    "%s -> %j",
    (code, expected) => {
      expect(translateMcpBackendError(ipcError(code), t)).toBe(expected);
    },
  );

  it("accepts the raw serialized payload as well as IpcError instances", () => {
    expect(
      translateMcpBackendError(
        { code: "TOML", message: "x", details: null, i18nKey: null },
        t,
      ),
    ).toBe("mcp.error.tomlInvalid");
  });

  it("maps by code regardless of the message wording", () => {
    // 文案改写（含英文/任意语言）不影响映射
    expect(
      translateMcpBackendError(
        ipcError("MCP_VALIDATION", "stdio server is missing command"),
        t,
      ),
    ).toBe("mcp.error.validationFailed");
  });

  it("does not parse message text of legacy string rejections", () => {
    // 尚未迁移的字符串错误：返回空串，由调用方回退到原始文案
    expect(
      translateMcpBackendError(
        "MCP 校验失败: stdio 类型的 MCP 服务器缺少 command 字段",
        t,
      ),
    ).toBe("");
    expect(translateMcpBackendError(new Error("MCP_VALIDATION"), t)).toBe("");
    expect(translateMcpBackendError(undefined, t)).toBe("");
  });
});
