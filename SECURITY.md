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

## Security Model / 安全模型

[docs/security/model.md](docs/security/model.md) states what CC Switch protects and what it does not: where data is stored and in what form, who can reach the local proxy, how deep links are confirmed, what cloud sync uploads, and how updates are signed. Release signing keys are covered by [docs/security/key-management.md](docs/security/key-management.md).

[docs/security/model.md](docs/security/model.md)（英文）说明了 CC Switch 保护什么、不保护什么：数据存放在哪里、以何种形式存放，谁可以访问本地代理，深度链接如何确认，云同步上传了什么，以及更新如何签名。发布签名密钥的管理见 [docs/security/key-management.md](docs/security/key-management.md)（英文）。

<a id="verifying-a-download"></a>

## Verifying a Download / 校验下载文件

Every release on the [Releases page](https://github.com/farion1231/cc-switch/releases) carries a `SHA256SUMS` file, a CycloneDX SBOM (`CC-Switch-<version>-sbom.cdx.json`) and GitHub build-provenance attestations for every asset.

每个版本都附带 `SHA256SUMS` 校验文件、CycloneDX 格式的 SBOM（`CC-Switch-<版本>-sbom.cdx.json`），并为每个文件生成 GitHub 构建来源证明（attestation）。

```bash
# 1. Checksum: run in the folder that holds the downloaded files
#    校验和：在下载文件所在目录运行
sha256sum -c SHA256SUMS --ignore-missing          # Linux
shasum -a 256 -c SHA256SUMS --ignore-missing      # macOS
# Windows (PowerShell): compare with the matching line in SHA256SUMS
# Windows（PowerShell）：与 SHA256SUMS 中对应的行比较
Get-FileHash .\CC-Switch-v3.17.0-Windows.msi -Algorithm SHA256

# 2. Provenance: proves the file was built by this repository's release workflow
#    来源证明：证明文件由本仓库的发布工作流构建
gh attestation verify CC-Switch-v3.17.0-Windows.msi --repo farion1231/cc-switch
```

The in-app updater does this automatically: it only installs updates whose minisign signature matches the public key built into the app. macOS builds are additionally signed and notarized by Apple; Windows builds are Authenticode-signed only when the maintainers' certificate is configured.

应用内更新会自动完成校验：只安装 minisign 签名与应用内置公钥匹配的更新。macOS 版本另有 Apple 签名与公证；Windows 版本仅在维护者配置了代码签名证书后才带 Authenticode 签名。

## Reporting Logs Safely / 安全地提交日志

Provider configs, `settings.json`, `auth.json`, `.env` files, proxy logs, debug-level app logs and screenshots of provider settings can contain API keys and tokens. Remove them before you attach anything to a public issue; if a key was posted, revoke it at the provider.

供应商配置、`settings.json`、`auth.json`、`.env`、代理日志、debug 级别的应用日志以及供应商设置的截图都可能包含 API 密钥和令牌。附加到公开 Issue 之前请先删除；如果密钥已经公开，请到供应商处吊销。

## Privacy and Data Flows / 隐私与数据流向

CC Switch has no telemetry or analytics. API keys, OAuth tokens and cloud-sync credentials are stored in `~/.cc-switch/` and in each tool's own config files. They are plaintext there unless you enable the optional keychain vault for provider API keys; WebDAV / S3 credentials are kept in the OS keychain where one is available ([docs/secrets-vault.md](docs/secrets-vault.md)). [PRIVACY.md](PRIVACY.md) lists what is stored locally and every network request the app makes (update check, provider requests through the local proxy, usage and subscription-quota queries that read CLI OAuth tokens, skills, cloud sync, and more). A request that is not listed there should be reported as a security issue.

CC Switch 不包含任何遥测或统计。API 密钥、OAuth 令牌和云同步凭据保存在 `~/.cc-switch/` 及各工具自己的配置文件中；除非启用可选的供应商 API 密钥钥匙串保险库，否则以明文保存；WebDAV / S3 凭据在系统提供钥匙串时保存在钥匙串中（[docs/secrets-vault.md](docs/secrets-vault.md)，英文）。[PRIVACY.md](PRIVACY.md)（英文）列出了本地存储的内容以及应用发出的所有网络请求（更新检查、经本地代理转发的供应商请求、读取 CLI OAuth 令牌的用量与订阅额度查询、技能、云同步等）。如发现未列出的请求，请按安全问题报告。
