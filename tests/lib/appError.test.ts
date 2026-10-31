/**
 * Structured backend errors (CMD-12, I18N-03): `AppError` serializes to
 * `{ code, message, params? }` and the frontend renders it by code.
 */
import fs from "node:fs";
import path from "node:path";
import i18n from "i18next";
import { afterEach, describe, expect, it } from "vitest";
import {
  APP_ERROR_DEFAULTS,
  BackendError,
  fallbackMessage,
  getErrorCode,
  isAppErrorPayload,
  localizeAppError,
  toBackendError,
} from "@/lib/errors/appError";
import { invoke, unwrap } from "@/lib/api/invoke";
import {
  extractErrorMessage,
  translateMcpBackendError,
} from "@/utils/errorUtils";
import en from "@/i18n/locales/en.json";
import { server } from "../msw/server";
import { http, HttpResponse } from "msw";

const ROOT = path.resolve(__dirname, "../..");
const CJK = /[㐀-鿿]/;

const localized = {
  code: "mcp.error.commandRequired",
  message:
    "stdio 类型的 MCP 服务器缺少 command 字段 (A stdio MCP server needs a command)",
  params: {
    zh: "stdio 类型的 MCP 服务器缺少 command 字段",
    en: "A stdio MCP server needs a command",
  },
};

const generic = {
  code: "errors.config",
  message: "配置错误: bad value",
  params: { detail: "bad value" },
};

afterEach(async () => {
  await i18n.changeLanguage("zh");
  i18n.removeResourceBundle("en", "translation");
  i18n.addResourceBundle("en", "translation", {});
});

describe("AppError payloads", () => {
  it("recognises the wire shape and nothing else", () => {
    expect(isAppErrorPayload(localized)).toBe(true);
    expect(isAppErrorPayload({ code: "x", message: "y", params: null })).toBe(
      true,
    );
    expect(isAppErrorPayload("errors.config")).toBe(false);
    expect(isAppErrorPayload(new Error("x"))).toBe(false);
    expect(isAppErrorPayload({ message: "no code" })).toBe(false);
  });

  it("falls back to the zh or en half that matches the UI language", () => {
    expect(fallbackMessage(localized, "zh")).toBe(localized.params.zh);
    expect(fallbackMessage(localized, "zh-TW")).toBe(localized.params.zh);
    expect(fallbackMessage(localized, "en")).toBe(localized.params.en);
    expect(fallbackMessage(localized, "ja")).toBe(localized.params.en);
  });

  it("renders generic variants with an English template outside zh", () => {
    expect(fallbackMessage(generic, "en")).toBe(
      "Configuration error: bad value",
    );
    expect(fallbackMessage(generic, "zh")).toBe("配置错误: bad value");
    expect(
      fallbackMessage(
        { code: "errors.message", message: "boom", params: { detail: "boom" } },
        "en",
      ),
    ).toBe("boom");
  });

  it("translates by code when the key exists", async () => {
    i18n.addResourceBundle("en", "translation", {
      mcp: { error: { commandRequired: "Please enter command" } },
    });
    await i18n.changeLanguage("en");
    expect(localizeAppError(localized)).toBe("Please enter command");
    // Untranslated key: the language-matched fallback, not the key.
    expect(
      localizeAppError({ ...localized, code: "mcp.error.notTranslatedYet" }),
    ).toBe(localized.params.en);
  });

  it("BackendError keeps the code and stringifies to the localized message", async () => {
    await i18n.changeLanguage("en");
    const err = new BackendError(generic);
    expect(err).toBeInstanceOf(Error);
    expect(err.code).toBe("errors.config");
    expect(err.message).toBe("Configuration error: bad value");
    expect(String(err)).toBe("Configuration error: bad value");
    expect(`${err}`).toBe("Configuration error: bad value");
    expect(extractErrorMessage(err)).toBe("Configuration error: bad value");
    expect(getErrorCode(err)).toBe("errors.config");
  });

  it("leaves legacy string errors untouched", () => {
    expect(toBackendError("plain failure")).toBe("plain failure");
    expect(getErrorCode("plain failure")).toBeUndefined();
    expect(extractErrorMessage("plain failure")).toBe("plain failure");
    const e = new Error("x");
    expect(toBackendError(e)).toBe(e);
  });

  it("extractErrorMessage localizes raw payloads", async () => {
    await i18n.changeLanguage("en");
    expect(extractErrorMessage(localized)).toBe(localized.params.en);
  });
});

describe("api invoke wrapper", () => {
  it("rethrows a structured rejection as a BackendError", async () => {
    // `unwrap` handles results from the generated bindings.
    expect(() =>
      unwrap({ status: "error", error: generic as unknown }),
    ).toThrow(BackendError);
    expect(unwrap({ status: "ok", data: 42 })).toBe(42);
    try {
      unwrap({ status: "error", error: "legacy" });
      expect.unreachable();
    } catch (e) {
      expect(e).toBe("legacy");
    }
  });

  it("passes invoke results through", async () => {
    server.use(
      http.post("http://tauri.local/get_config_dir", () =>
        HttpResponse.json("/tmp/x"),
      ),
    );
    await expect(invoke("get_config_dir", { app: "claude" })).resolves.toBe(
      "/tmp/x",
    );
  });
});

describe("MCP validation errors (I18N-03)", () => {
  it("maps backend codes to form messages, with no CJK in the en locale", async () => {
    i18n.addResourceBundle("en", "translation", en);
    await i18n.changeLanguage("en");
    const t = i18n.t.bind(i18n) as (key: string) => string;
    for (const code of [
      "mcp.error.idRequired",
      "mcp.error.jsonInvalid",
      "mcp.error.commandRequired",
      "mcp.wizard.urlRequired",
    ]) {
      const err = new BackendError({
        code,
        message: "中文 (English)",
        params: { zh: "中文", en: "English" },
      });
      const mapped = translateMcpBackendError(err, t);
      expect(mapped, code).not.toBe("");
      expect(CJK.test(mapped), `${code}: ${mapped}`).toBe(false);
      expect(CJK.test(err.message), `${code}: ${err.message}`).toBe(false);
    }
    expect(translateMcpBackendError("MCP 服务器 ID 不能为空", t)).toBe("");
    expect(translateMcpBackendError(new BackendError(generic), t)).toBe("");
  });

  it("errorUtils.ts contains no CJK literals", () => {
    const text = fs.readFileSync(
      path.join(ROOT, "src/utils/errorUtils.ts"),
      "utf8",
    );
    expect(CJK.test(text)).toBe(false);
  });

  it("every MCP key the backend emits exists in the en locale", () => {
    const sources = [
      "src-tauri/src/mcp/validation.rs",
      "src-tauri/src/claude_mcp.rs",
    ].map((f) => fs.readFileSync(path.join(ROOT, f), "utf8"));
    const keys = new Set<string>();
    for (const text of sources) {
      for (const m of text.matchAll(/AppError::localized\(\s*"([^"]+)"/g)) {
        keys.add(m[1]);
      }
    }
    expect(keys.size).toBeGreaterThan(0);
    i18n.addResourceBundle("en", "translation", en);
    for (const key of keys) {
      expect(i18n.exists(key, { lng: "en" }), key).toBe(true);
    }
  });
});

describe("Rust AppError codes", () => {
  it("every generic AppError code has an English default", () => {
    const errorRs = fs.readFileSync(
      path.join(ROOT, "src-tauri/src/error.rs"),
      "utf8",
    );
    const codes = [...errorRs.matchAll(/=> "(errors\.[A-Za-z]+)"/g)].map(
      (m) => m[1],
    );
    expect(codes.length).toBeGreaterThan(10);
    for (const code of codes) {
      expect(APP_ERROR_DEFAULTS[code], code).toBeTruthy();
    }
    expect(Object.keys(APP_ERROR_DEFAULTS).sort()).toEqual([...codes].sort());
  });
});

describe("localized backend errors without locale entries", () => {
  // Emitted via `AppError::localized` (zh/en travel in `params`), so they
  // render without an `errors.*` template or a locale key.
  const KEYS = [
    "proxy.port_unavailable",
    "provider.universal_child_in_use",
    "provider.live_sync_partial",
    "sync.remote_path.invalid",
    "s3.header.invalid",
    "env_profile.no_variables",
    "env_profile.unsupported_app",
  ];

  it("are still emitted by the backend", () => {
    const sources = [
      "src-tauri/src/services/proxy/mod.rs",
      "src-tauri/src/services/provider/mod.rs",
      "src-tauri/src/services/provider/live.rs",
      "src-tauri/src/services/sync_protocol.rs",
      "src-tauri/src/services/s3.rs",
      "src-tauri/src/services/env_profile.rs",
    ]
      .map((p) => fs.readFileSync(path.join(ROOT, p), "utf8"))
      .join("\n");
    for (const key of KEYS) expect(sources, key).toContain(`"${key}"`);
  });

  it("render the English half in an English UI and the Chinese half in zh", async () => {
    for (const code of KEYS) {
      const payload = {
        code,
        message: "中文 (English text)",
        params: { zh: "中文", en: "English text" },
      };
      await i18n.changeLanguage("en");
      expect(localizeAppError(payload), code).toBe("English text");
      await i18n.changeLanguage("zh");
      expect(localizeAppError(payload), code).toBe("中文");
    }
  });
});
