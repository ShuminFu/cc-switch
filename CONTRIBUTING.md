# Contributing to CC Switch

> [中文版本](#贡献指南)

Thank you for your interest in contributing to CC Switch! Please read our [Code of Conduct](./CODE_OF_CONDUCT.md) before participating.

## How to Contribute

There are many ways to contribute:

- **Report bugs** — Found something broken? [Open a bug report](https://github.com/farion1231/cc-switch/issues/new?template=bug_report.yml).
- **Suggest features** — Have an idea? [Submit a feature request](https://github.com/farion1231/cc-switch/issues/new?template=feature_request.yml).
- **Improve docs** — Spot a typo or missing info? [Report a doc issue](https://github.com/farion1231/cc-switch/issues/new?template=doc_issue.yml).
- **Contribute code** — Fix bugs or implement features via pull requests.
- **Translate** — Help us improve translations for English, Simplified Chinese, Traditional Chinese, and Japanese.

> **Security vulnerabilities**: Please do NOT use public issues. See our [Security Policy](./SECURITY.md) instead.

## Development Setup

### Prerequisites

- **Node.js** `>=22.12.0` (`package.json` engines; Vite 7 also accepts `^20.19.0`). `.node-version` pins `22.12.0`, which is what CI uses.
- **pnpm 10** — CI uses `10.12.3` (`pnpm-lock.yaml` is lockfile v9, which pnpm 8 cannot read). With corepack: `corepack prepare pnpm@10.12.3 --activate`.
- **Rust** via [rustup](https://rustup.rs/). You do not need to pick a version: `rust-toolchain.toml` pins channel `1.95` (with `rustfmt` and `clippy`), and rustup installs it automatically on the first `cargo` command.
- [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS (WebView2 on Windows, `webkit2gtk` etc. on Linux, Xcode Command Line Tools on macOS). On Debian/Ubuntu, `scripts/install-linux-deps.sh` installs exactly what CI installs. The Tauri CLI comes from `devDependencies` (`@tauri-apps/cli` 2.8+), so no global install is needed.

New here? [docs/dev/architecture.md](docs/dev/architecture.md) maps the codebase and lists the invariants every change must keep; [AGENTS.md](AGENTS.md) is the short rulebook (also used by the automated PR review).

### Quick Start

```bash
# Install dependencies
pnpm install

# Start development server with hot reload
pnpm dev
```

### Useful Commands

| Command | Description |
|---------|-------------|
| `pnpm dev` | Start dev server (hot reload) |
| `pnpm build` | Production build |
| `pnpm typecheck` | TypeScript type checking |
| `pnpm dev:renderer` | Start only the Vite frontend (no Tauri shell) |
| `pnpm test:unit` | Run unit tests (Vitest, `tests/`) |
| `pnpm test:unit:watch` | Run unit tests in watch mode |
| `pnpm lint` | ESLint check |
| `pnpm format` | Format code (Prettier: `src/**`, `scripts/**`, `.github/**`) |
| `pnpm format:check` | Check code formatting |
| `pnpm i18n:check` | Check the four locale files are complete and consistent |
| `pnpm check:repo` | Repository checks: labeler globs, release asset names in docs, README parity, image sizes |
| `pnpm test:coverage` | Unit tests with a coverage report in `coverage/` |
| `pnpm check` | Everything the CI frontend job runs, in one command |
| `pnpm check:rust` | Everything the CI backend job runs (creates `dist/` if missing) |
| `pnpm bundle:report` | Build the renderer and print JS/CSS sizes against the budget |

For Rust backend:

```bash
cd src-tauri
cargo fmt                                # Format Rust code
cargo clippy --all-targets --all-features -- -D warnings # Run linter (same flags as CI)
cargo test --locked                      # Run tests
```

`pnpm build` and `cargo` builds need a `dist/` directory at the repo root; run `pnpm build:renderer` (or `mkdir -p dist`) first if `cargo test` complains it is missing.

## Code Style

- **Frontend**: Prettier for formatting, ESLint for linting, strict TypeScript (`pnpm typecheck`)
- **Backend**: `cargo fmt` for formatting, `cargo clippy` for linting
- **Tauri commands**: command functions are `snake_case` in Rust (e.g. `get_stream_check_config`) and are invoked with that same name from the frontend. Argument keys on the JS side are camelCase (Tauri converts them; some commands set `#[tauri::command(rename_all = "camelCase")]` explicitly).

Run all checks before submitting (these are the same steps `.github/workflows/ci.yml` runs; see [docs/dev/testing.md](docs/dev/testing.md)):

```bash
pnpm check        # typecheck, lint, format:check, i18n:check, check:repo, test:unit
pnpm check:rust   # cargo fmt --check, clippy --all-targets --all-features -D warnings, cargo test --locked
```

## Pull Request Guidelines

1. **Open an issue first** for new features — PRs for features that are not a good fit may be closed.
2. **Fork and branch** — Create a feature branch from `main` (e.g., `feat/my-feature` or `fix/issue-123`).
3. **Keep PRs focused** — One feature or fix per PR. Avoid unrelated changes.
4. **Follow the PR template** — Fill in the summary, related issue, and checklist.

### PR Checklist

- [ ] PR title follows Conventional Commits (checked automatically)
- [ ] `pnpm check` passes
- [ ] `pnpm check:rust` passes (if Rust code changed)
- [ ] Tests added or updated
- [ ] Updated all four locale files if user-facing text changed
- [ ] User-visible change noted under `## [Unreleased]` in `CHANGELOG.md`

### Commit Convention

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(provider): add support for new provider
fix(tray): resolve menu not updating after switch
docs(readme): update installation instructions
ci: add format check workflow
chore(deps): update dependencies
```

## AI-Assisted Contributions

We welcome AI-assisted contributions, but **the responsibility stays with you**. AI tools lower the cost of writing code — they do not lower the cost of reviewing it. Maintainers are not obligated to clean up AI-generated output.

By submitting a PR, you agree to the following:

1. **You have read and understood your code.** You must be able to explain any line in your PR. If you cannot, it is not ready for review.
2. **You have tested it yourself.** Every change must be verified locally — not just "it looks right." Do not submit code for platforms or features you cannot test.
3. **PRs must be small and focused.** One issue, one PR. Large, sprawling, multi-topic PRs will be closed.
4. **Open an issue first.** Drive-by PRs with no prior discussion — especially AI-generated ones — may be closed without review.
5. **Maintainers may close without explanation.** PRs that appear to be unreviewed AI output — hallucinated fixes, unnecessary refactors, bulk changes with no context — may be closed at the maintainer's discretion.

**In short**: AI is a tool, not a substitute for understanding. Use it to help you contribute better, not to shift work onto maintainers.

## Internationalization (i18n)

CC Switch ships four UI languages. English is the source language and the fallback (`fallbackLng: "en"` in `src/i18n/index.ts`). When modifying user-facing text:

1. Add or change the key in **all four** locale files:
   - `src/i18n/locales/en.json` — English (source)
   - `src/i18n/locales/zh.json` — Simplified Chinese
   - `src/i18n/locales/zh-TW.json` — Traditional Chinese
   - `src/i18n/locales/ja.json` — Japanese
2. Use the `t()` function from react-i18next for all UI text, and never hardcode user-facing strings. If you cannot translate a string, copy the English text into the other files rather than leaving the key out, and say so in the PR.
3. Keep `{{placeholders}}`, brand and product names (Claude Code, Codex, Gemini CLI, OpenCode, ...), and Markdown/HTML tags unchanged in every translation.
4. Adding a new locale means adding a JSON file next to the others and registering it in `src/i18n/index.ts`; open an issue first.

## Releases

Maintainers cut releases with [RELEASING.md](RELEASING.md) (version bump, changelog, release notes, tag, package channels, rollback).

## Questions?

- [Open a question](https://github.com/farion1231/cc-switch/issues/new?template=question.yml)
- [GitHub Discussions](https://github.com/farion1231/cc-switch/discussions)

---

# 贡献指南

> [English Version](#contributing-to-cc-switch)

感谢你对 CC Switch 的贡献兴趣！参与之前请阅读我们的[行为准则](./CODE_OF_CONDUCT.md)。

## 如何贡献

你可以通过多种方式参与贡献：

- **报告 Bug** — 发现问题？[提交 Bug 报告](https://github.com/farion1231/cc-switch/issues/new?template=bug_report.yml)。
- **建议功能** — 有想法？[提交功能请求](https://github.com/farion1231/cc-switch/issues/new?template=feature_request.yml)。
- **改进文档** — 发现错误或缺失？[报告文档问题](https://github.com/farion1231/cc-switch/issues/new?template=doc_issue.yml)。
- **贡献代码** — 通过 Pull Request 修复 Bug 或实现新功能。
- **翻译** — 帮助改进英文、简体中文、繁体中文和日文的翻译。

> **安全漏洞**：请不要使用公开 Issue 报告。请参阅我们的[安全策略](./SECURITY.md)。

## 开发环境搭建

### 前提条件

- **Node.js** `>=22.12.0`（`package.json` engines；Vite 7 也支持 `^20.19.0`）。`.node-version` 固定为 `22.12.0`，CI 使用的就是这个版本。
- **pnpm 10** — CI 使用 `10.12.3`（`pnpm-lock.yaml` 为 v9 格式，pnpm 8 无法读取）。使用 corepack：`corepack prepare pnpm@10.12.3 --activate`。
- **Rust**：通过 [rustup](https://rustup.rs/) 安装即可，无需手动选择版本。`rust-toolchain.toml` 固定了 `1.95` 工具链（含 `rustfmt` 与 `clippy`），首次运行 `cargo` 时 rustup 会自动安装。
- 对应操作系统的 [Tauri 2 开发环境](https://v2.tauri.app/start/prerequisites/)（Windows 需 WebView2，Linux 需 `webkit2gtk` 等，macOS 需 Xcode Command Line Tools）。Debian/Ubuntu 上可运行 `scripts/install-linux-deps.sh`，安装的依赖与 CI 完全一致。Tauri CLI 已在 `devDependencies`（`@tauri-apps/cli` 2.8+）中，无需全局安装。

第一次参与？[docs/dev/architecture.md](docs/dev/architecture.md)（英文）介绍了代码结构以及每次改动都必须遵守的约束；[AGENTS.md](AGENTS.md)（英文）是简短的规则清单（自动 PR 审查也使用它）。

### 快速开始

```bash
# 安装依赖
pnpm install

# 启动开发服务器（热重载）
pnpm dev
```

### 常用命令

| 命令 | 说明 |
|------|------|
| `pnpm dev` | 启动开发服务器（热重载） |
| `pnpm build` | 构建生产版本 |
| `pnpm typecheck` | TypeScript 类型检查 |
| `pnpm dev:renderer` | 仅启动 Vite 前端（不含 Tauri 外壳） |
| `pnpm test:unit` | 运行单元测试（Vitest，`tests/`） |
| `pnpm test:unit:watch` | 以监听模式运行单元测试 |
| `pnpm lint` | ESLint 检查 |
| `pnpm format` | 格式化代码（Prettier：`src/**`、`scripts/**`、`.github/**`） |
| `pnpm format:check` | 检查代码格式 |
| `pnpm i18n:check` | 检查四个语言文件是否完整一致 |
| `pnpm check:repo` | 仓库检查：labeler 规则、文档中的发布文件名、README 结构一致性、图片大小 |
| `pnpm test:coverage` | 运行单元测试并在 `coverage/` 生成覆盖率报告 |
| `pnpm check` | 一条命令运行 CI 前端任务的全部检查 |
| `pnpm check:rust` | 一条命令运行 CI 后端任务的全部检查（缺少 `dist/` 时会自动创建） |
| `pnpm bundle:report` | 构建前端并按预算输出 JS/CSS 体积 |

Rust 后端命令：

```bash
cd src-tauri
cargo fmt                                # 格式化 Rust 代码
cargo clippy --all-targets --all-features -- -D warnings # 运行 Clippy 检查（与 CI 参数一致）
cargo test --locked                      # 运行测试
```

`pnpm build` 和 `cargo` 构建需要仓库根目录存在 `dist/` 目录；如果 `cargo test` 提示缺失，请先运行 `pnpm build:renderer`（或 `mkdir -p dist`）。

## 代码规范

- **前端**：使用 Prettier 格式化、ESLint 检查、严格 TypeScript（`pnpm typecheck`）
- **后端**：使用 `cargo fmt` 格式化、`cargo clippy` 检查
- **Tauri 命令**：Rust 中的命令函数使用 `snake_case`（如 `get_stream_check_config`），前端以相同名称调用；JS 侧的参数键使用 camelCase（Tauri 会自动转换，部分命令显式声明了 `#[tauri::command(rename_all = "camelCase")]`）。

提交前运行所有检查（与 `.github/workflows/ci.yml` 执行的步骤相同，详见 [docs/dev/testing.md](docs/dev/testing.md)，英文）：

```bash
pnpm check        # typecheck、lint、format:check、i18n:check、check:repo、test:unit
pnpm check:rust   # cargo fmt --check、clippy --all-targets --all-features -D warnings、cargo test --locked
```

## Pull Request 指南

1. **先开 Issue 讨论** — 新功能请先开 Issue，不适合项目方向的 PR 可能会被关闭。
2. **Fork 并创建分支** — 从 `main` 创建功能分支（如 `feat/my-feature` 或 `fix/issue-123`）。
3. **保持 PR 专注** — 每个 PR 只做一件事，避免无关改动。
4. **遵循 PR 模板** — 填写概述、关联 Issue 和检查清单。

### PR 检查清单

- [ ] PR 标题符合 Conventional Commits（会自动检查）
- [ ] `pnpm check` 通过
- [ ] `pnpm check:rust` 通过（如修改了 Rust 代码）
- [ ] 已添加或更新测试
- [ ] 如修改了用户可见文本，已同步更新四个语言文件
- [ ] 用户可见的改动已记录到 `CHANGELOG.md` 的 `## [Unreleased]`

### 提交信息规范

我们使用 [Conventional Commits](https://www.conventionalcommits.org/)：

```
feat(provider): add support for new provider
fix(tray): resolve menu not updating after switch
docs(readme): update installation instructions
ci: add format check workflow
chore(deps): update dependencies
```

## AI 辅助贡献

我们欢迎 AI 辅助的贡献，但**责任始终在你身上**。AI 工具降低了写代码的成本，但并没有降低 review 的成本。维护者没有义务替你清理 AI 的产出。

提交 PR 即表示你同意以下规则：

1. **你已阅读并理解了你的代码。** 你必须能解释 PR 中的每一行。如果做不到，说明还没准备好提交 review。
2. **你已亲自测试过。** 每个改动都必须在本地验证——而不是"看起来对"。不要提交你自己无法测试的平台或功能的代码。
3. **PR 必须小而聚焦。** 一个 Issue 对应一个 PR。大而散、跨多个主题的 PR 会被直接关闭。
4. **先开 Issue 讨论。** 没有事先讨论的"路过式 PR"——尤其是 AI 生成的——可能会被直接关闭。
5. **维护者可以直接关闭。** 看起来是未经审阅的 AI 产出的 PR——虚构的修复、不必要的重构、缺乏上下文的批量改动——维护者可自行决定关闭。

**一句话总结**：AI 是工具，不是理解力的替代品。用它来帮助你更好地贡献，而不是把工作转移给维护者。

## 国际化（i18n）

CC Switch 提供四种界面语言。英文是源语言，也是回退语言（`src/i18n/index.ts` 中的 `fallbackLng: "en"`）。修改用户可见文本时：

1. 在**全部四个**语言文件中新增或修改对应的键：
   - `src/i18n/locales/en.json` — 英文（源语言）
   - `src/i18n/locales/zh.json` — 简体中文
   - `src/i18n/locales/zh-TW.json` — 繁体中文
   - `src/i18n/locales/ja.json` — 日文
2. 所有 UI 文本使用 react-i18next 的 `t()` 函数，不要硬编码用户可见的字符串。如果无法翻译某条文本，请先把英文原文填入其他语言文件，不要缺少键，并在 PR 中注明。
3. 所有译文中保持 `{{占位符}}`、品牌与产品名称（Claude Code、Codex、Gemini CLI、OpenCode 等）以及 Markdown/HTML 标签不变。
4. 新增语言需要在同目录添加 JSON 文件，并在 `src/i18n/index.ts` 中注册；请先开 Issue 讨论。

## 发版

维护者按照 [RELEASING.md](RELEASING.md)（英文）发版：版本号、更新日志、发布说明、打标签、各安装渠道与回滚。

## 有疑问？

- [提问](https://github.com/farion1231/cc-switch/issues/new?template=question.yml)
- [GitHub 讨论区](https://github.com/farion1231/cc-switch/discussions)
