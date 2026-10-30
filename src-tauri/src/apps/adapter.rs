//! 按应用划分的配置适配器
//!
//! 每个受管应用实现一个 [`AppAdapter`]，把"这个应用的配置放在哪、长什么样、
//! 怎么读写"收拢到 `apps/<app>/` 一个模块里。调用方通过
//! [`super::adapter`] 取得适配器后统一分派，不再各自 `match AppType`。
//!
//! 可选能力（MCP 投影、代理接管、会话扫描）以子 trait 表达，由
//! [`AppAdapter::mcp`] / [`AppAdapter::takeover`] / [`AppAdapter::session_source`]
//! 返回；不支持的应用返回 `None`。

use std::fmt::Debug;
use std::path::PathBuf;

use serde_json::Value;

use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::omo::OmoVariant;

/// 供应商直连凭据
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub api_key: String,
    pub base_url: String,
}

/// `value` 为字符串时取其内容，否则返回空串
pub(crate) fn str_or_empty(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or("").to_string()
}

/// `keys` 中第一个存在且非空的字符串。
///
/// 对齐前端的 `a || b || c`：JS 的 `||` 会跳过空串，而预设会把
/// `ANTHROPIC_AUTH_TOKEN` 之类的字段写成"存在但为空"的占位，所以只跳过缺失键的
/// `.get().or_else()` 链会提前停下。
pub(crate) fn first_non_empty_str(env: Option<&Value>, keys: &[&str]) -> String {
    let Some(env) = env else {
        return String::new();
    };
    for key in keys {
        if let Some(s) = env.get(key).and_then(Value::as_str) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    String::new()
}

/// 单个应用的 live 配置适配器
pub trait AppAdapter: Debug + Sync {
    /// 应用配置目录（已考虑 settings.json 中的目录覆盖）
    fn config_dir(&self) -> Result<PathBuf, AppError>;

    /// 提示词文件路径；没有提示词文件的应用返回 `None`
    fn prompt_file(&self) -> Option<PathBuf>;

    /// Skills 目录；没有 skills 目录的应用返回 `None`
    fn skills_dir(&self) -> Option<PathBuf>;

    /// 写入供应商会改写的 live 文件（深链接导入预览据此提示）；
    /// 不经 [`AppAdapter::write_provider`] 直接写 live 的应用返回空列表
    fn live_files(&self) -> Vec<PathBuf>;

    /// 读取当前 live 配置，转换为供应商 `settings_config` 结构
    fn read_live(&self) -> Result<Value, AppError>;

    /// 首次导入时读取 live 配置作为 `default` 供应商的 `settings_config`
    fn import_live(&self) -> Result<Value, AppError>;

    /// 首次导入的 `default` 供应商分类（`provider.category`）
    fn import_category(&self, _settings: &Value) -> &'static str {
        "custom"
    }

    /// 把供应商配置写入 live 文件
    fn write_provider(&self, provider: &Provider) -> Result<(), AppError>;

    /// 写入已合并通用配置的有效供应商配置。
    ///
    /// 需要数据库上下文的应用（Claude Desktop 的网关令牌）覆盖此方法，其余应用
    /// 直接写 live。
    fn write_effective_provider(
        &self,
        _db: &Database,
        provider: &Provider,
    ) -> Result<(), AppError> {
        self.write_provider(provider)
    }

    /// 在有效配置上补齐只属于 live 的应用默认值（写 live 前调用）。
    ///
    /// 注入的内容必须由 [`AppAdapter::backfill_live_settings`] 对称剥掉。
    fn apply_live_defaults(&self, _provider: &Provider, _settings: &mut Value) {}

    /// 把旧版字段迁移为当前结构；返回是否有改动
    fn normalize_settings(&self, _settings: &mut Value) -> bool {
        false
    }

    /// 从 live 配置中移除供应商。
    ///
    /// 只有累加模式应用需要实现；切换模式应用的 live 只承载当前供应商，默认 no-op。
    fn remove_provider(&self, _provider_id: &str) -> Result<(), AppError> {
        Ok(())
    }

    /// live 配置里是否已有该供应商。
    ///
    /// 只有累加模式应用的 live 同时承载多个供应商；切换模式应用默认返回 `false`。
    fn live_has_provider(&self, _provider_id: &str) -> Result<bool, AppError> {
        Ok(false)
    }

    /// 切换到该供应商后更新 live 里的默认选择（Hermes 的顶层 `model:`）。
    ///
    /// 累加模式应用写入供应商条目并不会改变应用当前使用的供应商，需要的应用覆盖
    /// 此方法；失败由调用方降级为警告。
    fn apply_switch_defaults(&self, _provider: &Provider) -> Result<(), AppError> {
        Ok(())
    }

    /// 按分类独占写入专用配置文件的供应商变体（OpenCode 的 OMO / OMO Slim）。
    ///
    /// 返回 `Some` 的供应商不写入应用主配置，而是通过 [`crate::services::OmoService`]
    /// 激活 / 清除对应文件。
    fn omo_variant(&self, _category: Option<&str>) -> Option<&'static OmoVariant> {
        None
    }

    /// 校验供应商 `settings_config` 的结构
    fn validate_provider(&self, provider: &Provider) -> Result<(), AppError>;

    /// 切走时把 live 配置回填到供应商之前，剥掉只属于 live 的投影内容
    fn backfill_live_settings(&self, _provider: &Provider, live_settings: Value) -> Value {
        live_settings
    }

    /// 代理接管期间 live 是否由接管备份托管。
    ///
    /// 为 `true` 时，接管期间保存 / 切换供应商只更新备份；为 `false` 的应用
    ///（Claude Desktop 的 3P 配置）不经过接管备份，始终直接写入。
    fn takeover_uses_live_backup(&self) -> bool {
        true
    }

    /// 从供应商配置中提取直连凭据
    fn credentials(&self, provider: &Provider) -> Result<Credentials, AppError>;

    /// 用量查询（原生余额 / coding plan、JS 脚本的 `{{apiKey}}` / `{{baseUrl}}`）
    /// 使用的 `(base_url, api_key)`，缺失的字段为空串。
    ///
    /// 与 [`AppAdapter::credentials`] 不同，这里从不报错，并与前端
    /// `UsageScriptModal.tsx` 的 `getProviderCredentials` 保持一致。
    fn usage_credentials(&self, settings: &Value) -> (String, String);

    /// 通用配置片段支持；不支持的应用返回 `None`
    fn common_snippet(&self) -> Option<&dyn CommonSnippet> {
        None
    }

    /// CC Switch 管理的 MCP 服务器投影；不支持的应用返回 `None`
    fn mcp(&self) -> Option<&dyn McpProjector> {
        None
    }

    /// 本地代理接管；不支持的应用返回 `None`
    fn takeover(&self) -> Option<&dyn ProxyTakeover> {
        None
    }

    /// 会话历史来源；不支持的应用返回 `None`
    fn session_source(&self) -> Option<&dyn SessionSource> {
        None
    }
}

/// 通用配置片段的提取 / 判定 / 合并 / 剥离。
///
/// `snippet` 参数均为调用方已去除首尾空白且非空的片段文本。
pub trait CommonSnippet {
    /// 从供应商配置中提取可共享的片段（剥掉凭据与端点）
    fn extract(&self, settings: &Value) -> Result<String, AppError>;

    /// `settings` 是否已包含整个片段
    fn contains(&self, settings: &Value, snippet: &str) -> bool;

    /// 把片段合并进 `settings`
    fn apply(&self, settings: &Value, snippet: &str) -> Result<Value, AppError>;

    /// 从 `settings` 中剥掉片段
    fn remove(&self, settings: &Value, snippet: &str) -> Result<Value, AppError>;

    /// 切走供应商时是否从 live 重新提取片段，捕获用户在应用里直接做的改动
    fn sync_from_live(&self) -> bool {
        false
    }
}

/// 只支持提取、不参与合并的通用配置片段（OpenCode / OpenClaw）。
///
/// 这些累加模式应用可以从编辑器内容里提取片段，但写 live 时不合并片段：
/// `contains` 恒为 `false`，`apply` / `remove` 原样返回。
pub struct ExtractOnlySnippet(pub fn(&Value) -> Result<String, AppError>);

impl CommonSnippet for ExtractOnlySnippet {
    fn extract(&self, settings: &Value) -> Result<String, AppError> {
        (self.0)(settings)
    }

    fn contains(&self, _settings: &Value, _snippet: &str) -> bool {
        false
    }

    fn apply(&self, settings: &Value, _snippet: &str) -> Result<Value, AppError> {
        Ok(settings.clone())
    }

    fn remove(&self, settings: &Value, _snippet: &str) -> Result<Value, AppError> {
        Ok(settings.clone())
    }
}

/// 把 CC Switch 的 MCP 服务器写入 / 移出应用自己的配置，并从中读回
pub trait McpProjector {
    /// MCP 服务器写入的 live 配置文件
    fn live_file(&self) -> PathBuf;

    /// 读取应用 live 配置中的 MCP 服务器，已转换为统一 JSON 规范（尚未校验）。
    /// 配置文件缺失时返回空列表；文件损坏时报错。
    fn read_servers(&self) -> Result<Vec<(String, Value)>, AppError>;

    fn upsert(&self, id: &str, spec: &Value) -> Result<(), AppError>;

    fn remove(&self, id: &str) -> Result<(), AppError>;
}

/// 本地代理接管 live 配置的应用侧约定
pub trait ProxyTakeover {
    /// live / 备份配置是否带有代理接管占位符
    fn has_placeholder(&self, settings: &Value) -> bool;

    /// 官方供应商在接管期间是否仍可选用（默认拦截）
    fn official_provider_supported(&self, _provider: &Provider) -> bool {
        false
    }

    /// 接管时写入 live 的代理基础地址。
    ///
    /// `proxy_origin` 为 `http://host:port`（无尾部斜杠），`proxy_route` 为描述符中的
    /// [`crate::apps::registry::AppDescriptor::proxy_route`]。默认使用 `{route}/v1`
    /// 前缀路由；早于前缀路由接管的应用覆写此方法，保持已写入用户 live 的地址不变。
    fn base_url(&self, proxy_origin: &str, proxy_route: &str) -> String {
        format!("{proxy_origin}{proxy_route}/v1")
    }

    /// 累加模式应用的条目级接管；切换模式应用（接管整份 live）返回 `None`
    fn entry_takeover(&self) -> Option<&dyn EntryTakeover> {
        None
    }
}

/// 供应商条目的端点字段（缺失的字段为 `None`）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryEndpoint {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

/// 累加模式应用的条目级接管（OpenCode / OpenClaw / Hermes）。
///
/// 这些应用的 live 同时承载多个供应商条目，接管只改写应用当前选中条目的
/// 基础地址与 API Key，其余条目与配置保持原样。`live` 参数均为
/// [`AppAdapter::read_live`] 返回的整份配置。
pub trait EntryTakeover {
    /// live 中 CC Switch 可写的供应商条目（id, 条目）
    fn entries(&self, live: &Value) -> Vec<(String, Value)>;

    /// 应用默认模型引用的供应商条目 id（OpenCode `model`、OpenClaw
    /// `agents.defaults.model.primary`、Hermes `model.provider`）
    fn default_entry(&self, live: &Value) -> Option<String>;

    /// 条目（或供应商 `settings_config`）的端点字段
    fn endpoint(&self, entry: &Value) -> EntryEndpoint;

    /// 条目说的是否为本地代理挂载的 OpenAI 兼容协议（Chat Completions / Responses）
    fn proxyable(&self, entry: &Value) -> bool;

    /// 只改写 live 中该条目的端点字段；`None` 表示删除该字段
    fn write_endpoint(&self, id: &str, endpoint: &EntryEndpoint) -> Result<(), AppError>;
}

/// 把端点字段写入 JSON 对象：`Some` 覆盖（保留键位置），`None` 删除
pub(crate) fn apply_endpoint_field(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<&str>,
) {
    match value {
        Some(value) => {
            object.insert(key.to_string(), Value::String(value.to_string()));
        }
        None => {
            object.remove(key);
        }
    }
}

/// 端点字段是否为 CC Switch 写入的代理 token
pub(crate) fn endpoint_has_proxy_token(endpoint: &EntryEndpoint) -> bool {
    endpoint
        .api_key
        .as_deref()
        .is_some_and(crate::proxy::inbound_auth::is_proxy_token)
}

/// 累加模式应用的占位符判定：`settings` 可以是整份 live（任一条目带 token）
/// 或单个条目 / 供应商 `settings_config`
pub(crate) fn entries_have_placeholder(entries: &dyn EntryTakeover, settings: &Value) -> bool {
    endpoint_has_proxy_token(&entries.endpoint(settings))
        || entries
            .entries(settings)
            .iter()
            .any(|(_, entry)| endpoint_has_proxy_token(&entries.endpoint(entry)))
}

/// 会话历史扫描入口
pub trait SessionSource {
    /// 存放会话文件的根目录（删除会话时据此做路径校验）
    fn session_roots(&self) -> Vec<PathBuf>;
}
