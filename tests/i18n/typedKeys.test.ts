import i18n from "i18next";
import { describe, it, expect } from "vitest";
import type { TranslationKey } from "@/i18n/keys";

// CCS-199：src/i18n/i18next.d.ts 让 t() 的 key 参数由 en.json 推导。
// 下面的 @ts-expect-error 由 `pnpm typecheck` 校验：一旦类型声明失效（未知 key 不再报错），
// 该指令变成“未使用”，typecheck 随之失败。运行时部分只确认测试文件本身可执行。
describe("typed i18n keys", () => {
  it("accepts keys from en.json and rejects unknown ones at compile time", () => {
    const known: TranslationKey = "common.save";
    // @ts-expect-error -- 不存在于 en.json 的 key 不能赋值给 TranslationKey
    const unknown: TranslationKey = "common.definitelyNotARealKey";

    expect(i18n.t(known)).toBe("common.save");
    // @ts-expect-error -- 未知 key 直接传给 t() 同样无法通过类型检查
    expect(i18n.t("common.definitelyNotARealKey")).toBe(unknown);
  });
});
