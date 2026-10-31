import { useState, useCallback, useEffect, useRef } from "react";
import TOML from "smol-toml";
import i18n from "i18next";
import { normalizeTomlText } from "@/utils/textNormalization";

/**
 * Codex config.toml 格式校验 Hook
 * 使用 smol-toml 进行实时 TOML 语法校验（带 debounce）
 */
export function useCodexTomlValidation() {
  const [configError, setConfigError] = useState("");
  // True when the text only fails because smart quotes are used as string
  // delimiters; the editor offers a one-click fix instead of rewriting the
  // buffer while the user types (FE-ARCH-37).
  const [smartQuoteFixAvailable, setSmartQuoteFixAvailable] = useState(false);
  const debounceTimerRef = useRef<NodeJS.Timeout | null>(null);

  /**
   * 校验 TOML 格式
   * @param tomlText - 待校验的 TOML 文本
   * @returns 是否校验通过
   */
  const validateToml = useCallback((tomlText: string): boolean => {
    // 空字符串视为合法（允许为空）
    if (!tomlText.trim()) {
      setConfigError("");
      setSmartQuoteFixAvailable(false);
      return true;
    }

    try {
      TOML.parse(tomlText);
      setConfigError("");
      setSmartQuoteFixAvailable(false);
      return true;
    } catch (error) {
      const errorMessage =
        error instanceof Error
          ? error.message
          : i18n.t("codexConfig.tomlInvalid", {
              defaultValue: "Invalid TOML",
            });
      let fixable = false;
      const fixed = normalizeTomlText(tomlText);
      if (fixed !== tomlText) {
        try {
          TOML.parse(fixed);
          fixable = true;
        } catch {
          fixable = false;
        }
      }
      setConfigError(errorMessage);
      setSmartQuoteFixAvailable(fixable);
      return false;
    }
  }, []);

  /**
   * 带 debounce 的校验函数（500ms 延迟）
   * @param tomlText - 待校验的 TOML 文本
   */
  const debouncedValidate = useCallback(
    (tomlText: string) => {
      // 清除之前的定时器
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }

      // 设置新的定时器
      debounceTimerRef.current = setTimeout(() => {
        validateToml(tomlText);
      }, 500);
    },
    [validateToml],
  );

  /**
   * 清空错误信息
   */
  const clearError = useCallback(() => {
    setConfigError("");
    setSmartQuoteFixAvailable(false);
  }, []);

  // 清理定时器
  useEffect(() => {
    return () => {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }
    };
  }, []);

  return {
    configError,
    smartQuoteFixAvailable,
    validateToml,
    debouncedValidate,
    clearError,
  };
}
