import i18n from "i18next";
import { afterAll, beforeAll } from "vitest";
import zh from "@/i18n/locales/zh.json";

/**
 * 全局测试环境的 i18n 资源为空（t() 返回 key）。需要断言真实中文文案（含插值结果）的
 * 测试文件在 describe 内调用本函数，临时加载 zh 语言包，结束后恢复为空资源。
 */
export const loadZhTranslations = () => {
  beforeAll(() => {
    i18n.addResourceBundle("zh", "translation", zh, true, true);
  });
  afterAll(() => {
    i18n.removeResourceBundle("zh", "translation");
    i18n.addResourceBundle("zh", "translation", {});
  });
};
