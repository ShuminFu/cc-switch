# CC Switch 用户手册

> Claude Code / Claude Desktop / Codex / Gemini CLI / Grok Build / OpenCode / OpenClaw / Hermes 全方位辅助工具

## 目录结构

```
📚 CC Switch 用户手册
│
├── 1. 快速入门
│   ├── 1.1 软件介绍
│   ├── 1.2 安装指南
│   ├── 1.3 界面概览
│   ├── 1.4 快速上手
│   ├── 1.5 个性化配置
│   ├── 1.6 项目
│   └── 1.7 各应用支持的功能
│
├── 2. 供应商管理
│   ├── 2.1 添加供应商
│   ├── 2.2 切换供应商
│   ├── 2.3 编辑供应商
│   ├── 2.4 排序与复制
│   ├── 2.5 用量查询
│   ├── 2.6 Claude Desktop
│   └── 2.7 Grok Build
│
├── 3. 扩展功能
│   ├── 3.1 MCP 服务器管理
│   ├── 3.2 Prompts 提示词管理
│   ├── 3.3 Skills 技能管理
│   ├── 3.4 会话管理器
│   └── 3.5 工作区文件与每日记忆
│
├── 4. 代理与高可用
│   ├── 4.1 代理服务
│   ├── 4.2 应用接管
│   ├── 4.3 故障转移
│   ├── 4.4 用量统计
│   ├── 4.5 模型检查
│   └── 4.6 代理行为说明
│
└── 5. 常见问题
    ├── 5.1 配置文件说明
    ├── 5.2 FAQ
    ├── 5.3 深度链接协议
    ├── 5.4 环境变量冲突
    └── 5.6 故障排查
```

## 文件列表

### 1. 快速入门

| 文件 | 内容 |
|------|------|
| [1.1-introduction.md](./1-getting-started/1.1-introduction.md) | 软件介绍、核心功能、支持平台 |
| [1.2-installation.md](./1-getting-started/1.2-installation.md) | Windows/macOS/Linux 安装指南 |
| [1.3-interface.md](./1-getting-started/1.3-interface.md) | 界面布局、导航栏、供应商卡片说明 |
| [1.4-quickstart.md](./1-getting-started/1.4-quickstart.md) | 5 分钟快速上手教程 |
| [1.5-settings.md](./1-getting-started/1.5-settings.md) | 语言、主题、目录、备份、WebDAV / S3 云同步 |
| [1.6-projects.md](./1-getting-started/1.6-projects.md) | 项目：保存并切换供应商 / MCP / Skills / 提示词快照 |
| [1.7-capabilities.md](./1-getting-started/1.7-capabilities.md) | 各功能（MCP、提示词、Skills、接管、项目、深度链接）适用于哪些应用 |

### 2. 供应商管理

| 文件 | 内容 |
|------|------|
| [2.1-add.md](./2-providers/2.1-add.md) | 使用预设、自定义配置、统一供应商 |
| [2.2-switch.md](./2-providers/2.2-switch.md) | 主界面切换、托盘切换、生效方式 |
| [2.3-edit.md](./2-providers/2.3-edit.md) | 编辑配置、修改 API Key、回填机制 |
| [2.4-sort-duplicate.md](./2-providers/2.4-sort-duplicate.md) | 拖拽排序、复制供应商、删除 |
| [2.5-usage-query.md](./2-providers/2.5-usage-query.md) | 用量查询、剩余额度、多套餐显示 |
| [2.6-claude-desktop.md](./2-providers/2.6-claude-desktop.md) | Claude Desktop 第三方供应商、直连与模型映射 |
| [2.7-grok-build.md](./2-providers/2.7-grok-build.md) | Grok Build 供应商、`config.toml`、代理接管 |

### 3. 扩展功能

| 文件 | 内容 |
|------|------|
| [3.1-mcp.md](./3-extensions/3.1-mcp.md) | MCP 协议、添加服务器、应用绑定 |
| [3.2-prompts.md](./3-extensions/3.2-prompts.md) | 创建预设、激活切换、智能回填 |
| [3.3-skills.md](./3-extensions/3.3-skills.md) | 发现技能、安装卸载、仓库管理 |
| [3.4-sessions.md](./3-extensions/3.4-sessions.md) | 会话浏览、搜索过滤、恢复与删除 |
| [3.5-workspace.md](./3-extensions/3.5-workspace.md) | OpenClaw 工作区文件、每日记忆 |

### 4. 代理与高可用

| 文件 | 内容 |
|------|------|
| [4.1-service.md](./4-proxy/4.1-service.md) | 启动代理、配置项、局域网访问令牌、运行状态 |
| [4.2-routing.md](./4-proxy/4.2-routing.md) | 应用路由、配置修改、状态指示 |
| [4.3-failover.md](./4-proxy/4.3-failover.md) | 故障转移队列、熔断器、健康状态 |
| [4.4-usage.md](./4-proxy/4.4-usage.md) | 用量统计、趋势图表、定价配置 |
| [4.5-model-test.md](./4-proxy/4.5-model-test.md) | 模型检查、健康检测、延迟测试 |
| [4.6-semantics.md](./4-proxy/4.6-semantics.md) | 哪些错误会故障转移、超时、熔断规则、接管对磁盘的修改 |

### 5. 常见问题

| 文件 | 内容 |
|------|------|
| [5.1-config-files.md](./5-faq/5.1-config-files.md) | CC Switch 存储、CLI 配置文件格式 |
| [5.2-questions.md](./5-faq/5.2-questions.md) | 常见问题解答 |
| [5.3-deeplink.md](./5-faq/5.3-deeplink.md) | 深度链接协议、生成和使用方法 |
| [5.4-env-conflict.md](./5-faq/5.4-env-conflict.md) | 环境变量冲突检测与处理 |
| [5.6-troubleshooting.md](./5-faq/5.6-troubleshooting.md) | 各系统的故障排查、日志与崩溃日志位置、如何提交 Bug |

## 快速链接

- **新用户**：从 [1.1 软件介绍](./1-getting-started/1.1-introduction.md) 开始
- **安装问题**：查看 [1.2 安装指南](./1-getting-started/1.2-installation.md)
- **配置供应商**：查看 [2.1 添加供应商](./2-providers/2.1-add.md)
- **使用 Claude Desktop**：查看 [2.6 Claude Desktop](./2-providers/2.6-claude-desktop.md)
- **使用 Grok Build**：查看 [2.7 Grok Build](./2-providers/2.7-grok-build.md)
- **一键切换整套配置**：查看 [1.6 项目](./1-getting-started/1.6-projects.md)
- **使用代理**：查看 [4.1 代理服务](./4-proxy/4.1-service.md)
- **遇到问题**：查看 [5.2 FAQ](./5-faq/5.2-questions.md)

## 版本信息

- 文档版本：v3.17.0
- 最后更新：2026-10-30
- 适用于 CC Switch v3.17.0+

### v3.17.0 之后的变更（下一版本）

- **局域网访问令牌**：代理监听非回环地址时，其他设备必须携带每次安装生成的访问令牌；面板会显示警告和令牌 — 详见 [4.1 代理服务](./4-proxy/4.1-service.md)
- **深度链接需主动确认**：深度链接不再自动切换供应商、启用 MCP 服务器或启用用量脚本，除非你勾选对应复选框；MCP 导入会显示完整命令 — 详见 [5.3 深度链接协议](./5-faq/5.3-deeplink.md)
- **同步冲突检测**：WebDAV / S3 上传和下载不会覆盖上次同步后其他设备或本机所做的修改；明文 `http://` 地址会被标记 — 详见 [1.5 个性化配置](./1-getting-started/1.5-settings.md)
- **备份类型**：数据库备份按类型命名（定时、手动、安全、迁移前）并分别轮换；手动备份永不自动删除；恢复前先校验 — 详见 [1.5 个性化配置](./1-getting-started/1.5-settings.md)
- **Live 文件安全**：覆盖 live 配置文件前会先备份，无法解析的文件会中止切换而不是被替换 — 详见 [5.1 配置文件说明](./5-faq/5.1-config-files.md)

### v3.17.0 亮点

- **项目（Project Profiles）**：把 Claude Code、Codex 或 Claude Desktop 的供应商、MCP、Skills 与提示词状态保存为命名项目，并从顶部栏或托盘一键切换 — 详见 [1.6 项目](./1-getting-started/1.6-projects.md)
- **Grok Build**：一级支持 xAI 的 Grok Build CLI，包括供应商、MCP、Skills、提示词、会话与代理接管 — 详见 [2.7 Grok Build](./2-providers/2.7-grok-build.md)
- **Codex 官方会话走代理**：ChatGPT 订阅的 Codex 官方会话可通过本地代理接管路由；Codex 供应商还可以对接原生 Anthropic Messages 上游 — 详见 [2.1 添加供应商](./2-providers/2.1-add.md)

### v3.16.0 亮点

- **Codex Chat Completions 路由**：DeepSeek、Kimi、GLM、MiniMax 等仅支持 Chat 协议的供应商可通过 Codex 使用 — 详见 [2.1 添加供应商](./2-providers/2.1-add.md)
- **托管 CLI 工具生命周期**：在设置 / 关于页安装、升级、全部升级并诊断 Claude / Codex / Gemini / OpenCode / OpenClaw / Hermes — 详见 [1.5 个性化配置](./1-getting-started/1.5-settings.md)
- **供应商与模型矩阵刷新**：新增合作方预设，刷新默认模型与计费矩阵，Claude Opus 默认升级到 4.8，适用场景下 GPT 默认升级到 5.5
- **路由支持徽章**：Claude Code / Codex 供应商卡片会标明是否支持 Local Routing，便于选择可代理的供应商
- **Codex OAuth 实时模型发现**：ChatGPT Codex 类供应商按需从 ChatGPT 后端拉取最新模型列表
- **用量看板筛选驱动 Hero**：展示缓存归一化后的真实总 token 与缓存命中率，并跟随日期 / 供应商 / 模型筛选实时更新 — 详见 [4.4 用量统计](./4-proxy/4.4-usage.md)
- **轻量模式**：退出到托盘时销毁主窗口，空闲占用接近零 — 详见 [1.5 个性化配置](./1-getting-started/1.5-settings.md)
- **配额与余额展示**：官方订阅类（Claude/Codex/Gemini/Copilot/Codex OAuth）自动展示剩余额度；Token Plan 和第三方余额通过内置模板一键启用 — 详见 [2.5 用量查询](./2-providers/2.5-usage-query.md)
- **Codex OAuth 反向代理**：用 ChatGPT 账号在 Claude Code 中复用 Codex 服务 — 详见 [2.1 添加供应商](./2-providers/2.1-add.md)
- **托盘按应用分级菜单**：Claude / Codex / Gemini 独立子菜单，标题展示当前供应商与可用用量摘要 — 详见 [2.2 切换供应商](./2-providers/2.2-switch.md)
- **Skills 发现与批量更新**：SHA-256 更新检测、批量更新、skills.sh 公共注册表搜索 — 详见 [3.3 Skills 技能管理](./3-extensions/3.3-skills.md)
- **完整 URL 端点模式**：高级选项支持将 base_url 视作完整上游端点 — 详见 [2.1 添加供应商](./2-providers/2.1-add.md)
- **OpenCode / OpenClaw / Hermes 流式检测覆盖**：Stream Check 面板覆盖 Claude / Codex / Gemini / OpenCode / OpenClaw / Hermes — 详见 [4.5 模型检查](./4-proxy/4.5-model-test.md)

## 贡献

欢迎提交 Issue 或 PR 改进文档：

- [GitHub Issues](https://github.com/farion1231/cc-switch/issues)
- [GitHub Repository](https://github.com/farion1231/cc-switch)
