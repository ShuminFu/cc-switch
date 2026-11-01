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

- [ ] PR title follows Conventional Commits (`feat(scope): ...`, `fix(scope): ...`) / PR 标题符合 Conventional Commits 规范
- [ ] `pnpm check` passes (typecheck, lint, format, i18n, repo checks, unit tests) / 通过 `pnpm check`
- [ ] `pnpm check:rust` passes if Rust code changed (fmt, clippy `-D warnings`, `cargo test`) / 如修改了 Rust 代码，通过 `pnpm check:rust`
- [ ] Regression test added for every fix, or N/A because: ___ / 每个修复都附带回归测试，或说明不适用的原因
- [ ] User-facing text: all four locale files updated (`en`, `zh`, `zh-TW`, `ja`) / 用户可见文本已同步更新四个语言文件
- [ ] User-visible change noted under `## [Unreleased]` in `CHANGELOG.md` / 用户可见的改动已记录到 `CHANGELOG.md` 的 `## [Unreleased]`
- [ ] Docs updated if behaviour changed (user manual, README) / 行为变化时已更新文档
