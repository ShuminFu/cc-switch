import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TFunction } from "i18next";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import {
  IPC_ERROR_CODES,
  IpcError,
  getIpcErrorCode,
  invoke,
  isIpcErrorPayload,
} from "@/lib/api/ipc";
import {
  formatSkillError,
  parseSkillError,
} from "@/lib/errors/skillErrorParser";
import { extractErrorMessage } from "@/utils/errorUtils";

const t = ((key: string, opts?: Record<string, string>) =>
  opts ? `${key} ${JSON.stringify(opts)}` : key) as unknown as TFunction;

const payload = (code: string, extra: Record<string, unknown> = {}) => ({
  code,
  message: `${code} message`,
  details: null,
  i18nKey: null,
  ...extra,
});

describe("ipc invoke wrapper", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("forwards arguments unchanged and resolves the result", async () => {
    invokeMock.mockResolvedValue(42);
    await expect(invoke<number>("get_x")).resolves.toBe(42);
    expect(invokeMock).toHaveBeenCalledWith("get_x");
    await invoke("set_x", { value: 1 });
    expect(invokeMock).toHaveBeenLastCalledWith("set_x", { value: 1 });
  });

  it("turns a structured rejection into an IpcError that prints its message", async () => {
    invokeMock.mockImplementation(() =>
      Promise.reject(
        payload("CIRCUIT_OPEN", { message: "所有供应商已熔断，无可用渠道" }),
      ),
    );
    const error = (await invoke("start_proxy_server").catch(
      (e: unknown) => e,
    )) as IpcError;
    expect(error).toBeInstanceOf(IpcError);
    expect(error.code).toBe("CIRCUIT_OPEN");
    expect(String(error)).toBe("所有供应商已熔断，无可用渠道");
    expect(`${error}`).toBe("所有供应商已熔断，无可用渠道");
    expect(extractErrorMessage(error)).toBe("所有供应商已熔断，无可用渠道");
    expect(getIpcErrorCode(error)).toBe("CIRCUIT_OPEN");
  });

  it("keeps legacy string rejections as strings", async () => {
    invokeMock.mockImplementation(() => Promise.reject("legacy"));
    await expect(invoke("legacy_cmd")).rejects.toBe("legacy");
    expect(getIpcErrorCode("legacy")).toBeNull();
  });

  it("recognises every error code payload", () => {
    for (const code of IPC_ERROR_CODES) {
      expect(isIpcErrorPayload(payload(code))).toBe(true);
      expect(getIpcErrorCode(payload(code))).toBe(code);
    }
    expect(isIpcErrorPayload({ message: "no code" })).toBe(false);
  });

  it("carries the i18n key of a localized error", () => {
    const error = new IpcError(
      payload("LOCALIZED", { i18nKey: "usage.invalidPrice" }) as never,
    );
    expect(error.i18nKey).toBe("usage.invalidPrice");
  });
});

describe("skill errors switch on code", () => {
  it("translates a skill error from its details", () => {
    const error = new IpcError(
      payload("DOWNLOAD_TIMEOUT", {
        details: {
          context: { owner: "o", name: "n", timeout: "60" },
          suggestion: "checkNetwork",
        },
      }) as never,
    );
    expect(parseSkillError(error)).toEqual({
      code: "DOWNLOAD_TIMEOUT",
      context: { owner: "o", name: "n", timeout: "60" },
      suggestion: "checkNetwork",
    });
    const { title, description } = formatSkillError(error, t);
    expect(title).toBe("skills.installFailed");
    expect(description).toBe(
      'skills.error.downloadTimeout {"owner":"o","name":"n","timeout":"60"}\n\nskills.error.suggestion.checkNetwork',
    );
  });

  it("does not parse message text for codes", () => {
    // 文案里出现错误码不再触发翻译，只有 code 字段算数
    const { description } = formatSkillError(
      '{"code":"SKILL_NOT_FOUND","context":{}}',
      t,
    );
    expect(description).toBe('{"code":"SKILL_NOT_FOUND","context":{}}');
  });

  it("falls back to the message for non-skill codes", () => {
    const error = new IpcError(payload("DATABASE") as never);
    expect(parseSkillError(error)).toBeNull();
    expect(formatSkillError(error, t, "skills.updateFailed")).toEqual({
      title: "skills.updateFailed",
      description: "DATABASE message",
    });
  });
});
