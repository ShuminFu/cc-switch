## Summary / 概述

<!-- Briefly describe what this PR does and why. / 简要描述这个 PR 做了什么以及为什么。 -->

## Related Issue / 关联 Issue

<!-- Link the related issue. Use "Fixes #123" to auto-close it when merged. -->
<!-- 关联相关 Issue。使用 "Fixes #123" 可在合并时自动关闭。 -->

Fixes #

## Screenshots / 截图

<!-- If applicable, add before/after screenshots. / 如有需要，请添加修改前后的截图。 -->

| Before / 修改前 | After / 修改后 |
|-----------------|---------------|
|                 |               |

## Checklist / 检查清单

- [ ] `pnpm typecheck` passes / 通过 TypeScript 类型检查
- [ ] `pnpm format:check` passes / 通过代码格式检查
- [ ] `cargo clippy` passes (if Rust code changed) / 通过 Clippy 检查（如修改了 Rust 代码）
- [ ] Proxy change (`src-tauri/src/proxy/**`): added or updated a golden fixture under `src-tauri/tests/fixtures/proxy/` (a bug fix adds the fixture first), or a maintainer applied `no-fixture-needed` — see `docs/dev/ACCEPTANCE.md` / 修改了代理代码：已在 `src-tauri/tests/fixtures/proxy/` 下新增或更新 golden fixture（修复 bug 时先加 fixture），或由维护者添加 `no-fixture-needed` 标签
- [ ] Schema change (`src-tauri/src/database/schema.rs`, `SCHEMA_VERSION`, sync `db.sql` format): follows `docs/dev/DB_COMPATIBILITY.md` (additive within a `DB_COMPAT_VERSION`, migration fixture added, compat version bumped if not additive) / 修改了数据库 schema：遵循 `docs/dev/DB_COMPATIBILITY.md`（同一 `DB_COMPAT_VERSION` 内仅做增量变更、已添加迁移 fixture、非增量变更已提升兼容版本）
- [ ] Structural change keeps the old path callable until parity is shown / 结构性改动在验证等价前保留旧路径可调用
- [ ] Updated i18n files if user-facing text changed / 如修改了用户可见文本，已更新国际化文件
