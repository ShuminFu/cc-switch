# Security Policy / 安全策略

## Supported Versions / 支持的版本

Only the latest release of CC Switch receives security updates.

仅最新版本的 CC Switch 会收到安全更新。

| Version / 版本 | Supported / 是否支持 |
|----------------|---------------------|
| Latest 3.x     | ✅ Yes / 是          |
| < 3.0          | ❌ No / 否           |

## Reporting a Vulnerability / 报告漏洞

**Please do NOT report security vulnerabilities through public GitHub issues.**

**请不要通过公开的 GitHub Issue 报告安全漏洞。**

Instead, please report them through [GitHub Security Advisories](https://github.com/farion1231/cc-switch/security/advisories/new).

请通过 [GitHub 安全公告](https://github.com/farion1231/cc-switch/security/advisories/new) 进行报告。

When reporting, please include:

报告时请包含以下信息：

- A description of the vulnerability / 漏洞描述
- Steps to reproduce / 复现步骤
- Potential impact / 潜在影响
- Affected versions / 受影响版本

## Response Timeline / 响应时间

- **Acknowledgment / 确认**: within 48 hours / 48 小时内
- **Initial assessment / 初步评估**: within 7 days / 7 天内
- **Fix for critical issues / 关键问题修复**: within 14 days / 14 天内

## Disclosure Policy / 披露政策

We follow a coordinated disclosure process:

我们遵循协调披露流程：

1. The reporter submits the vulnerability privately. / 报告者私下提交漏洞。
2. We confirm and work on a fix. / 我们确认并修复漏洞。
3. A patch release is published. / 发布修复版本。
4. The vulnerability is publicly disclosed. / 公开披露漏洞详情。

Reporters will be credited in the release notes unless they prefer to remain anonymous.

除非报告者希望匿名，否则将在发布说明中致谢。

## Security Updates / 安全更新

Security fixes are released as patch versions and announced via [GitHub Releases](https://github.com/farion1231/cc-switch/releases). We recommend always updating to the latest version.

安全修复通过补丁版本发布，并通过 [GitHub Releases](https://github.com/farion1231/cc-switch/releases) 通知。建议始终更新到最新版本。

## Privacy and Data Flows / 隐私与数据流向

CC Switch has no telemetry or analytics. API keys, OAuth tokens and cloud-sync credentials are stored unencrypted in `~/.cc-switch/` and in each tool's own config files. [PRIVACY.md](PRIVACY.md) lists what is stored locally and every network request the app makes (update check, provider requests through the local proxy, usage and subscription-quota queries that read CLI OAuth tokens, skills, cloud sync, and more). A request that is not listed there should be reported as a security issue.

CC Switch 不包含任何遥测或统计。API 密钥、OAuth 令牌和云同步凭据以未加密形式保存在 `~/.cc-switch/` 及各工具自己的配置文件中。[PRIVACY.md](PRIVACY.md)（英文）列出了本地存储的内容以及应用发出的所有网络请求（更新检查、经本地代理转发的供应商请求、读取 CLI OAuth 令牌的用量与订阅额度查询、技能、云同步等）。如发现未列出的请求，请按安全问题报告。
