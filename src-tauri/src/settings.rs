use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use crate::app_config::AppType;
use crate::apps::registry;
use crate::error::AppError;
use crate::proxy::codec::CodecEngine;
use crate::secret_store::{self, SecretPolicy};
use crate::services::skill::{SkillStorageLocation, SyncMethod};
use crate::services::sync_protocol::SyncKey;

/// 自定义端点配置（历史兼容，实际存储在 provider.meta.custom_endpoints）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CustomEndpoint {
    pub url: String,
    pub added_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used: Option<i64>,
}

/// 主页面显示的应用配置
///
/// 按应用规范标识（注册表 slug）存储；序列化时为每个已注册应用写出一个布尔键，
/// 与旧版按字段定义的结构体 JSON 形状一致，旧版本读取新文件不受影响。
/// 未知键（更新版本新增的应用）原样保留，降级再升级不会丢失。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VisibleApps {
    /// 用户显式设置过的值；缺失的应用取注册表 `default_visible`
    entries: BTreeMap<String, bool>,
}

impl VisibleApps {
    /// Check if the specified app is visible
    pub fn is_visible(&self, app: &AppType) -> bool {
        let descriptor = registry::descriptor(app);
        self.entries
            .get(descriptor.slug)
            .copied()
            .unwrap_or(descriptor.default_visible)
    }

    /// 设置指定应用是否显示
    pub fn set_visible(&mut self, app: &AppType, visible: bool) {
        self.entries
            .insert(registry::descriptor(app).slug.to_string(), visible);
    }
}

impl Serialize for VisibleApps {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let unknown = self
            .entries
            .iter()
            .filter(|(key, _)| registry::find_by_id(key).is_none());
        let mut map = serializer.serialize_map(None)?;
        for descriptor in registry::APPS {
            map.serialize_entry(descriptor.slug, &self.is_visible(&descriptor.app))?;
        }
        for (key, visible) in unknown {
            map.serialize_entry(key, visible)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for VisibleApps {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = BTreeMap::<String, bool>::deserialize(deserializer)?;
        Ok(Self {
            entries: canonicalize_app_keys(raw),
        })
    }
}

/// 把按应用分槽的键经注册表别名归一到规范 slug（如 `claudeDesktop` → `claude-desktop`）。
/// 未注册的键原样保留；规范键与别名同时出现时规范键优先。
fn canonicalize_app_keys<T>(raw: BTreeMap<String, T>) -> BTreeMap<String, T> {
    let mut canonical = BTreeMap::new();
    let mut from_alias = BTreeMap::new();
    for (key, value) in raw {
        match registry::find_by_id(&key.trim().to_lowercase()) {
            Some(descriptor) if descriptor.slug == key => {
                canonical.insert(key, value);
            }
            Some(descriptor) => {
                from_alias.insert(descriptor.slug.to_string(), value);
            }
            None => {
                canonical.insert(key, value);
            }
        }
    }
    for (key, value) in from_alias {
        canonical.entry(key).or_insert(value);
    }
    canonical
}

/// 旧版扁平配置目录键（`slug` → camelCase 键）。只为旧版本兼容读写保留，
/// 新应用只写入 `configDirs` 映射，不在这里登记。
const LEGACY_CONFIG_DIR_KEYS: &[(&str, &str)] = &[
    ("claude", "claudeConfigDir"),
    ("codex", "codexConfigDir"),
    ("gemini", "geminiConfigDir"),
    ("grokbuild", "grokConfigDir"),
    ("opencode", "opencodeConfigDir"),
    ("openclaw", "openclawConfigDir"),
    ("hermes", "hermesConfigDir"),
];

/// 旧版扁平当前供应商键（`slug` → camelCase 键），同上只为兼容保留
const LEGACY_CURRENT_PROVIDER_KEYS: &[(&str, &str)] = &[
    ("claude", "currentProviderClaude"),
    ("claude-desktop", "currentProviderClaudeDesktop"),
    ("codex", "currentProviderCodex"),
    ("gemini", "currentProviderGemini"),
    ("grokbuild", "currentProviderGrokbuild"),
    ("opencode", "currentProviderOpencode"),
    ("openclaw", "currentProviderOpenclaw"),
    ("hermes", "currentProviderHermes"),
];

/// 按应用分槽的设备级状态：配置目录覆盖与当前供应商 ID。
///
/// 磁盘格式为以注册表 slug 为键的 `configDirs` / `currentProviders` 映射；
/// 为了让降级后的旧版本仍能找到当前供应商与目录覆盖，序列化时同时写出
/// 旧版扁平键（`claudeConfigDir`、`currentProviderClaude` ...），保留一个版本。
/// 读取时映射优先，映射中缺失的应用再回落到旧扁平键（旧版本保存过的文件
/// 只有扁平键，因为旧结构体会丢弃它不认识的映射）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PerAppSettings {
    /// 设备级配置目录覆盖（键为应用 slug）
    pub config_dirs: BTreeMap<String, String>,
    /// 当前供应商 ID（键为应用 slug；本地存储，优先于数据库 is_current）
    pub current_providers: BTreeMap<String, String>,
}

impl PerAppSettings {
    /// 去掉旧扁平键已覆盖的应用在映射中的条目，使扁平键（无论是否存在）成为
    /// 这些应用的唯一来源。用于前端整体保存：前端按旧扁平键编辑，回传的
    /// 映射是读取时的旧值，若映射优先则用户的修改（含清空）会被吞掉。
    pub fn strip_legacy_map_entries(value: &mut serde_json::Value) {
        let Some(obj) = value.as_object_mut() else {
            return;
        };
        for (map_key, legacy) in [
            ("configDirs", LEGACY_CONFIG_DIR_KEYS),
            ("currentProviders", LEGACY_CURRENT_PROVIDER_KEYS),
        ] {
            if let Some(map) = obj.get_mut(map_key).and_then(|m| m.as_object_mut()) {
                let keys: Vec<String> = map.keys().cloned().collect();
                for key in keys {
                    let slug = registry::find_by_id(&key.trim().to_lowercase())
                        .map(|d| d.slug)
                        .unwrap_or(key.as_str());
                    if legacy.iter().any(|(legacy_slug, _)| *legacy_slug == slug) {
                        map.remove(&key);
                    }
                }
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PerAppSettingsRepr {
    #[serde(default)]
    config_dirs: Option<BTreeMap<String, String>>,
    #[serde(default)]
    current_providers: Option<BTreeMap<String, String>>,
    #[serde(flatten)]
    rest: BTreeMap<String, serde_json::Value>,
}

fn merge_legacy_keys(
    map: Option<BTreeMap<String, String>>,
    legacy: &[(&str, &str)],
    rest: &BTreeMap<String, serde_json::Value>,
) -> BTreeMap<String, String> {
    let mut merged = canonicalize_app_keys(map.unwrap_or_default());
    for (slug, key) in legacy {
        if let Some(value) = rest.get(*key).and_then(|v| v.as_str()) {
            merged
                .entry((*slug).to_string())
                .or_insert_with(|| value.to_string());
        }
    }
    merged
}

impl<'de> Deserialize<'de> for PerAppSettings {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let repr = PerAppSettingsRepr::deserialize(deserializer)?;
        Ok(Self {
            config_dirs: merge_legacy_keys(repr.config_dirs, LEGACY_CONFIG_DIR_KEYS, &repr.rest),
            current_providers: merge_legacy_keys(
                repr.current_providers,
                LEGACY_CURRENT_PROVIDER_KEYS,
                &repr.rest,
            ),
        })
    }
}

impl Serialize for PerAppSettings {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        if !self.config_dirs.is_empty() {
            map.serialize_entry("configDirs", &self.config_dirs)?;
        }
        if !self.current_providers.is_empty() {
            map.serialize_entry("currentProviders", &self.current_providers)?;
        }
        // 旧版扁平键双写（保留一个版本，供降级后的旧版本读取）
        for (values, legacy) in [
            (&self.config_dirs, LEGACY_CONFIG_DIR_KEYS),
            (&self.current_providers, LEGACY_CURRENT_PROVIDER_KEYS),
        ] {
            for (slug, key) in legacy {
                if let Some(value) = values.get(*slug) {
                    map.serialize_entry(key, value)?;
                }
            }
        }
        map.end()
    }
}

/// WebDAV 同步状态（持久化同步进度信息）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WebDavSyncStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sync_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_remote_etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_local_manifest_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_remote_manifest_hash: Option<String>,
    /// 已与该远端同步过签名快照：之后拒绝未签名的快照（防降级），清除同步密钥时复位
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub signed_sync: bool,
}

fn default_remote_root() -> String {
    "cc-switch-sync".to_string()
}
fn default_profile() -> String {
    "default".to_string()
}

/// WebDAV 同步设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavSyncSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub auto_sync: bool,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_remote_root")]
    pub remote_root: String,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default)]
    pub status: WebDavSyncStatus,
}

impl Default for WebDavSyncSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_sync: false,
            base_url: String::new(),
            username: String::new(),
            password: String::new(),
            remote_root: default_remote_root(),
            profile: default_profile(),
            status: WebDavSyncStatus::default(),
        }
    }
}

impl WebDavSyncSettings {
    pub fn validate(&self) -> Result<(), crate::error::AppError> {
        if self.base_url.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "webdav.base_url.required",
                "WebDAV 地址不能为空",
                "WebDAV URL is required.",
            ));
        }
        if self.username.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "webdav.username.required",
                "WebDAV 用户名不能为空",
                "WebDAV username is required.",
            ));
        }
        Ok(())
    }

    pub fn normalize(&mut self) {
        self.base_url = self.base_url.trim().to_string();
        self.username = self.username.trim().to_string();
        self.remote_root = self.remote_root.trim().to_string();
        self.profile = self.profile.trim().to_string();
        if self.remote_root.is_empty() {
            self.remote_root = default_remote_root();
        }
        if self.profile.is_empty() {
            self.profile = default_profile();
        }
    }

    /// Returns true if all credential fields are blank (no config to persist).
    fn is_empty(&self) -> bool {
        self.base_url.is_empty() && self.username.is_empty() && self.password.is_empty()
    }
}

/// S3 同步设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct S3SyncSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub auto_sync: bool,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub access_key_id: String,
    #[serde(default)]
    pub secret_access_key: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default = "default_remote_root")]
    pub remote_root: String,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default)]
    pub status: WebDavSyncStatus,
}

impl Default for S3SyncSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_sync: false,
            region: String::new(),
            bucket: String::new(),
            access_key_id: String::new(),
            secret_access_key: String::new(),
            endpoint: String::new(),
            remote_root: default_remote_root(),
            profile: default_profile(),
            status: WebDavSyncStatus::default(),
        }
    }
}

impl S3SyncSettings {
    pub fn validate(&self) -> Result<(), crate::error::AppError> {
        if self.bucket.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "s3.bucket.required",
                "S3 存储桶不能为空",
                "S3 bucket is required.",
            ));
        }
        if self.region.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "s3.region.required",
                "S3 区域不能为空",
                "S3 region is required.",
            ));
        }
        if self.access_key_id.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "s3.access_key_id.required",
                "S3 Access Key ID 不能为空",
                "S3 Access Key ID is required.",
            ));
        }
        if self.secret_access_key.trim().is_empty() {
            return Err(crate::error::AppError::localized(
                "s3.secret_access_key.required",
                "S3 Secret Access Key 不能为空",
                "S3 Secret Access Key is required.",
            ));
        }
        Ok(())
    }

    pub fn normalize(&mut self) {
        self.region = self.region.trim().to_string();
        self.bucket = self.bucket.trim().to_string();
        self.access_key_id = self.access_key_id.trim().to_string();
        self.endpoint = self.endpoint.trim().to_string();
        self.remote_root = self.remote_root.trim().to_string();
        self.profile = self.profile.trim().to_string();
        if self.remote_root.is_empty() {
            self.remote_root = default_remote_root();
        }
        if self.profile.is_empty() {
            self.profile = default_profile();
        }
    }

    /// Returns true if all credential fields are blank (no config to persist).
    fn is_empty(&self) -> bool {
        self.bucket.is_empty()
            && self.region.is_empty()
            && self.access_key_id.is_empty()
            && self.secret_access_key.is_empty()
    }
}

/// 本机自动迁移状态。
///
/// 这里记录的是本机启动时执行过的一次性迁移；标记不随数据库同步。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalMigrations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_third_party_history_provider_bucket_v1:
        Option<CodexThirdPartyHistoryProviderBucketMigration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_provider_template_v1: Option<CodexProviderTemplateMigration>,
    /// 统一会话开关的官方历史迁移标记。开关关闭时会被清除，
    /// 这样重新开启能把"关闭期间"落入 openai 桶的官方会话补迁进来。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_official_history_unify_v1: Option<CodexOfficialHistoryUnifyMigration>,
    /// 用户删除过的内置模型定价 ID（P1 的删除标记）。v18 起删除以 `model_pricing`
    /// 墓碑为准，启动同步时这些 ID 会并入墓碑；这里仍双写，供旧版本与旧快照使用。
    /// 用户重新添加同名定价时移除。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted_builtin_model_pricing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexThirdPartyHistoryProviderBucketMigration {
    pub completed_at: String,
    pub target_provider_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_provider_ids: Vec<String>,
    #[serde(default)]
    pub migrated_jsonl_files: usize,
    #[serde(default)]
    pub migrated_state_rows: usize,
    #[serde(default)]
    pub scanned_history_files: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderTemplateMigration {
    pub completed_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migrated_provider_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOfficialHistoryUnifyMigration {
    pub completed_at: String,
    pub target_provider_id: String,
    #[serde(default)]
    pub migrated_jsonl_files: usize,
    #[serde(default)]
    pub migrated_state_rows: usize,
    /// 迁移时的规范化 Codex 目录。标记只对同一目录生效：
    /// 切换 codex_config_dir 后旧标记不会挡住新目录的迁移。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_config_dir: Option<String>,
}

/// 应用设置结构
///
/// 存储设备级别设置，保存在本地 `~/.cc-switch/settings.json`，不随数据库同步。
/// 这确保了云同步场景下多设备可以独立运作。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    // ===== 设备级 UI 设置 =====
    #[serde(default = "default_show_in_tray")]
    pub show_in_tray: bool,
    #[serde(default = "default_minimize_to_tray_on_close")]
    pub minimize_to_tray_on_close: bool,
    #[serde(default)]
    pub use_app_window_controls: bool,
    /// 是否启用 Claude 插件联动
    #[serde(default)]
    pub enable_claude_plugin_integration: bool,
    /// 是否跳过 Claude Code 初次安装确认
    #[serde(default)]
    pub skip_claude_onboarding: bool,
    /// 是否开机自启
    #[serde(default)]
    pub launch_on_startup: bool,
    /// 静默启动（程序启动时不显示主窗口，仅托盘运行）
    #[serde(default)]
    pub silent_startup: bool,
    /// 是否在主页面启用本地代理功能（默认关闭）
    #[serde(default)]
    pub enable_local_proxy: bool,
    /// User has confirmed the local proxy first-run notice
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_confirmed: Option<bool>,
    /// User has confirmed the usage query first-run notice
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_confirmed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_dashboard_refresh_interval_ms: Option<u32>,
    /// Whether to show the failover toggle independently on the main page
    #[serde(default)]
    pub enable_failover_toggle: bool,
    /// Keep each client session on the provider that last served it while that
    /// provider's breaker is available, so upstream prompt caches survive a
    /// failover. Only applies when auto failover is on. Opt-in: defaults to false.
    #[serde(default)]
    pub sticky_session_routing: bool,
    /// Protocol conversion engine for proxy bridges (`codecEngine`): `legacy`
    /// keeps the original per-bridge converters, `ir` uses the IR codecs for
    /// bridges that have been migrated. Kept for one release as a fallback.
    #[serde(default, skip_serializing_if = "CodecEngine::is_legacy")]
    pub codec_engine: CodecEngine,
    /// Whether to show the project profile switcher on the main page header
    #[serde(default = "default_show_profile_switcher")]
    pub show_profile_switcher: bool,
    /// Keep Codex ChatGPT login material in auth.json when switching to third-party providers.
    /// Opt-in: defaults to false so third-party switches cleanly overwrite auth.json.
    #[serde(default)]
    pub preserve_codex_official_auth_on_switch: bool,
    /// Run official Codex providers under the shared "custom" model_provider id
    /// so official sessions share one resume-history bucket with third-party
    /// providers. Opt-in: defaults to false.
    #[serde(default)]
    pub unify_codex_session_history: bool,
    /// User opted in (via the enable dialog checkbox) to migrate existing
    /// official sessions ("openai" bucket) into the shared bucket. Persisted so
    /// a failed migration retries at startup; cleared when the toggle turns off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unify_codex_migrate_existing: Option<bool>,
    /// User has confirmed the failover toggle first-run notice
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failover_confirmed: Option<bool>,
    /// User has confirmed the first-run welcome notice
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_run_notice_confirmed: Option<bool>,
    /// User has confirmed the common config first-run notice
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub common_config_confirmed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    // ===== 主页面显示的应用 =====
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_apps: Option<VisibleApps>,

    // ===== 按应用分槽的设备级状态（目录覆盖、当前供应商 ID）=====
    #[serde(flatten)]
    pub per_app: PerAppSettings,

    // ===== Skill 同步设置 =====
    /// Skill 同步方式：auto（默认，优先 symlink）、symlink、copy
    #[serde(default)]
    pub skill_sync_method: SyncMethod,
    /// Skill 存储位置：cc_switch（默认）或 unified（~/.agents/skills/）
    #[serde(default)]
    pub skill_storage_location: SkillStorageLocation,

    // ===== WebDAV 同步设置 =====
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webdav_sync: Option<WebDavSyncSettings>,

    // ===== S3 同步设置 =====
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s3_sync: Option<S3SyncSettings>,

    // ===== WebDAV 备份设置（旧版，保留向后兼容）=====
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webdav_backup: Option<serde_json::Value>,

    // ===== 备份策略设置 =====
    /// Auto-backup interval in hours (default 24, 0 = disabled)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_interval_hours: Option<u32>,
    /// Maximum number of backup files to retain (default 10)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_retain_count: Option<u32>,

    // ===== 数据保留策略（见 [`RetentionPolicy`]）=====
    /// Days of per-request usage detail to keep before it is rolled up (default 30, minimum 1)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_detail_retention_days: Option<u32>,
    /// Days of daily usage rollups to keep (default / 0 = keep forever)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_rollup_retention_days: Option<u32>,

    // ===== 终端设置 =====
    /// 首选终端应用（可选，默认使用系统默认终端）
    /// - macOS: "terminal" | "iterm2" | "warp" | "alacritty" | "kitty" | "ghostty" | "wezterm" | "kaku"
    /// - Windows: "cmd" | "powershell" | "wt" (Windows Terminal)
    /// - Linux: "gnome-terminal" | "konsole" | "xfce4-terminal" | "alacritty" | "kitty" | "ghostty"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_terminal: Option<String>,

    // ===== 本机自动迁移状态 =====
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_migrations: Option<LocalMigrations>,
}

fn default_show_in_tray() -> bool {
    true
}

fn default_minimize_to_tray_on_close() -> bool {
    true
}

fn default_show_profile_switcher() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            show_in_tray: true,
            minimize_to_tray_on_close: true,
            use_app_window_controls: false,
            enable_claude_plugin_integration: false,
            skip_claude_onboarding: false,
            launch_on_startup: false,
            silent_startup: false,
            enable_local_proxy: false,
            proxy_confirmed: None,
            usage_confirmed: None,
            usage_dashboard_refresh_interval_ms: None,
            enable_failover_toggle: false,
            sticky_session_routing: false,
            codec_engine: CodecEngine::Legacy,
            show_profile_switcher: true,
            preserve_codex_official_auth_on_switch: false,
            unify_codex_session_history: false,
            unify_codex_migrate_existing: None,
            failover_confirmed: None,
            first_run_notice_confirmed: None,
            common_config_confirmed: None,
            language: None,
            visible_apps: None,
            per_app: PerAppSettings::default(),
            skill_sync_method: SyncMethod::default(),
            skill_storage_location: SkillStorageLocation::default(),
            webdav_sync: None,
            s3_sync: None,
            webdav_backup: None,
            backup_interval_hours: None,
            backup_retain_count: None,
            usage_detail_retention_days: None,
            usage_rollup_retention_days: None,
            preferred_terminal: None,
            local_migrations: None,
        }
    }
}

impl AppSettings {
    /// 该应用的配置目录覆盖（原始值，未解析 `~`）
    pub fn config_dir(&self, app: &AppType) -> Option<&str> {
        self.per_app
            .config_dirs
            .get(registry::descriptor(app).slug)
            .map(String::as_str)
    }

    /// 设置或清除（`None`）该应用的配置目录覆盖
    pub fn set_config_dir(&mut self, app: &AppType, dir: Option<String>) {
        let slug = registry::descriptor(app).slug.to_string();
        match dir {
            Some(dir) => self.per_app.config_dirs.insert(slug, dir),
            None => self.per_app.config_dirs.remove(&slug),
        };
    }

    /// 构造用：返回设置了该应用配置目录覆盖的副本
    pub fn with_config_dir(mut self, app: &AppType, dir: impl Into<String>) -> Self {
        self.set_config_dir(app, Some(dir.into()));
        self
    }

    /// 该应用在本机记录的当前供应商 ID
    pub fn current_provider_id(&self, app: &AppType) -> Option<&str> {
        self.per_app
            .current_providers
            .get(registry::descriptor(app).slug)
            .map(String::as_str)
    }

    /// 设置或清除（`None`）该应用在本机记录的当前供应商 ID
    pub fn set_current_provider_id(&mut self, app: &AppType, id: Option<String>) {
        let slug = registry::descriptor(app).slug.to_string();
        match id {
            Some(id) => self.per_app.current_providers.insert(slug, id),
            None => self.per_app.current_providers.remove(&slug),
        };
    }

    /// 解析前端 `save_settings` 传入的设置。前端仍按旧扁平键
    /// （`claudeConfigDir`、`currentProviderClaude` ...）编辑并回传读取时的
    /// 映射旧值，因此这些应用以扁平键为准，见 [`PerAppSettings::strip_legacy_map_entries`]。
    pub fn from_frontend_value(mut value: serde_json::Value) -> Result<Self, serde_json::Error> {
        PerAppSettings::strip_legacy_map_entries(&mut value);
        serde_json::from_value(value)
    }

    fn settings_path() -> Option<PathBuf> {
        // settings.json 保留用于旧版本迁移和无数据库场景
        Some(crate::paths::current().settings_path())
    }

    fn normalize_paths(&mut self) {
        self.per_app.config_dirs = std::mem::take(&mut self.per_app.config_dirs)
            .into_iter()
            .filter_map(|(app, dir)| {
                let trimmed = dir.trim();
                (!trimmed.is_empty()).then(|| (app, trimmed.to_string()))
            })
            .collect();

        self.language = self
            .language
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| matches!(*s, "en" | "zh" | "zh-TW" | "ja"))
            .map(|s| s.to_string());

        if let Some(sync) = &mut self.webdav_sync {
            sync.normalize();
            if sync.is_empty() {
                self.webdav_sync = None;
            }
        }

        if let Some(s3) = &mut self.s3_sync {
            s3.normalize();
            if s3.is_empty() {
                self.s3_sync = None;
            }
        }
    }

    /// 从 settings.json 加载设置
    ///
    /// 文件不存在时返回默认值；文件存在但无法读取或解析时同样返回默认值，
    /// 并附带 [`SettingsLoadFailure`]：解析失败会先把原始字节备份到
    /// `settings.json.corrupt-<ts>`，避免下一次保存静默覆盖 WebDAV/S3 凭据、
    /// 当前供应商和 `local_migrations` 标记。
    ///
    /// WebDAV 密码与 S3 Secret Access Key 从 [`crate::secret_store`] 补全；返回的
    /// `bool` 表示文件仍需重写（明文凭据待迁移，或降级模式下需写回明文）。
    fn load_from_file() -> (Self, Option<SettingsLoadFailure>, bool) {
        let Some(path) = Self::settings_path() else {
            return (Self::default(), None, false);
        };
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return (Self::default(), None, false);
            }
            Err(err) => {
                log::error!(
                    "读取设置文件失败，将使用默认设置且禁止覆盖该文件。路径: {}, 错误: {}",
                    path.display(),
                    err
                );
                let failure = SettingsLoadFailure {
                    path,
                    error: err.to_string(),
                    backup_path: None,
                };
                return (Self::default(), Some(failure), false);
            }
        };
        match serde_json::from_slice::<AppSettings>(&bytes) {
            Ok(mut settings) => {
                settings.normalize_paths();
                let secrets_pending = settings.hydrate_secrets(&settings_secret_policy(&path));
                (settings, None, secrets_pending)
            }
            Err(err) => {
                let backup_path = match backup_corrupt_settings(&path, &bytes) {
                    Ok(backup) => {
                        log::warn!(
                            "解析设置文件失败，将使用默认设置；原文件已备份。路径: {}, 备份: {}, 错误: {}",
                            path.display(),
                            backup.display(),
                            err
                        );
                        Some(backup)
                    }
                    Err(backup_err) => {
                        log::error!(
                            "解析设置文件失败且备份失败，将使用默认设置并禁止覆盖该文件。路径: {}, 错误: {}, 备份错误: {}",
                            path.display(),
                            err,
                            backup_err
                        );
                        None
                    }
                };
                let failure = SettingsLoadFailure {
                    path,
                    error: err.to_string(),
                    backup_path,
                };
                (Self::default(), Some(failure), false)
            }
        }
    }

    /// 从凭据存储补全为空的 WebDAV/S3 凭据，返回文件是否需要重写
    fn hydrate_secrets(&mut self, policy: &SecretPolicy) -> bool {
        let mut pending = false;
        if let Some(sync) = &mut self.webdav_sync {
            pending |= policy.hydrate(secret_store::WEBDAV_PASSWORD_KEY, &mut sync.password);
        }
        if let Some(s3) = &mut self.s3_sync {
            pending |= policy.hydrate(
                secret_store::S3_SECRET_ACCESS_KEY,
                &mut s3.secret_access_key,
            );
        }
        pending
    }

    /// 把 WebDAV/S3 凭据写入凭据存储，并把将要落盘的字段置空（写入失败或降级模式下保留明文）。
    /// 对应配置已删除或凭据已清空时同时删除存储中的旧值。
    fn externalize_secrets(&mut self, policy: &SecretPolicy) {
        let password = self
            .webdav_sync
            .as_ref()
            .map(|sync| sync.password.clone())
            .unwrap_or_default();
        let inline = policy.externalize(secret_store::WEBDAV_PASSWORD_KEY, &password);
        if let Some(sync) = &mut self.webdav_sync {
            sync.password = inline;
        }

        let secret = self
            .s3_sync
            .as_ref()
            .map(|s3| s3.secret_access_key.clone())
            .unwrap_or_default();
        let inline = policy.externalize(secret_store::S3_SECRET_ACCESS_KEY, &secret);
        if let Some(s3) = &mut self.s3_sync {
            s3.secret_access_key = inline;
        }
    }
}

/// settings.json 所在目录对应的凭据策略
fn settings_secret_policy(settings_path: &Path) -> SecretPolicy {
    secret_store::policy_for_dir(settings_path.parent().unwrap_or_else(|| Path::new(".")))
}

/// settings.json 加载失败的记录（供前端提示用户）
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsLoadFailure {
    /// 加载失败的 settings.json 路径
    pub path: PathBuf,
    /// 读取或解析错误信息
    pub error: String,
    /// 原始内容的备份路径；为 `None` 时表示未能备份，保存会被拒绝直到用户确认
    pub backup_path: Option<PathBuf>,
}

impl SettingsLoadFailure {
    /// 原文件未备份时禁止覆盖，以免丢失其中的凭据与迁移标记
    fn blocks_save(&self) -> bool {
        self.backup_path.is_none()
    }
}

/// 把无法解析的 settings.json 原样备份到 `settings.json.corrupt-<ts>`（仅属主可读写）
fn backup_corrupt_settings(path: &Path, bytes: &[u8]) -> Result<PathBuf, AppError> {
    let file_name = path
        .file_name()
        .ok_or_else(|| AppError::Config("无效的文件名".to_string()))?
        .to_string_lossy()
        .to_string();
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let base = path.with_file_name(format!("{file_name}.corrupt-{ts}"));
    let mut backup = base.clone();
    let mut n = 1;
    while fs::symlink_metadata(&backup).is_ok() {
        backup = PathBuf::from(format!("{}-{n}", base.display()));
        n += 1;
    }
    crate::config::atomic_write_secret(&backup, bytes)?;
    Ok(backup)
}

fn save_settings_file(store: &SettingsStore, settings: &AppSettings) -> Result<(), AppError> {
    if let Some(failure) = store
        .load_failure()
        .filter(SettingsLoadFailure::blocks_save)
    {
        return Err(AppError::Config(format!(
            "设置文件 {} 加载失败且未能备份（{}），为避免覆盖其中的数据已拒绝保存；请修复该文件或确认重置后重试",
            failure.path.display(),
            failure.error
        )));
    }

    let mut normalized = settings.clone();
    normalized.normalize_paths();
    let Some(path) = AppSettings::settings_path() else {
        return Err(AppError::Config("无法获取用户主目录".to_string()));
    };

    // WebDAV/S3 凭据移入系统钥匙串（或 0600 的 secrets.json），文件中仅留空字段
    let policy = settings_secret_policy(&path);
    normalized.externalize_secrets(&policy);

    let json = serde_json::to_string_pretty(&normalized)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    // 钥匙串写入失败或降级模式下文件仍可能含凭据：原子写入并强制仅属主可访问（unix 0600）
    crate::config::atomic_write_secret(&path, json.as_bytes())?;
    policy.release([
        secret_store::WEBDAV_PASSWORD_KEY,
        secret_store::S3_SECRET_ACCESS_KEY,
    ]);
    store.set_secrets_pending(false);
    Ok(())
}

/// 进程级设置缓存及其加载状态
struct SettingsStore {
    settings: RwLock<AppSettings>,
    load_failure: RwLock<Option<SettingsLoadFailure>>,
    /// 文件中的凭据与当前凭据策略不一致（明文待迁移，或降级模式待写回）
    secrets_pending: AtomicBool,
}

impl SettingsStore {
    fn load() -> Self {
        let (settings, load_failure, secrets_pending) = AppSettings::load_from_file();
        Self {
            settings: RwLock::new(settings),
            load_failure: RwLock::new(load_failure),
            secrets_pending: AtomicBool::new(secrets_pending),
        }
    }

    fn secrets_pending(&self) -> bool {
        self.secrets_pending.load(Ordering::SeqCst)
    }

    fn set_secrets_pending(&self, pending: bool) {
        self.secrets_pending.store(pending, Ordering::SeqCst);
    }

    fn load_failure(&self) -> Option<SettingsLoadFailure> {
        self.load_failure
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn set_load_failure(&self, failure: Option<SettingsLoadFailure>) {
        *self.load_failure.write().unwrap_or_else(|e| e.into_inner()) = failure;
    }
}

impl std::ops::Deref for SettingsStore {
    type Target = RwLock<AppSettings>;

    fn deref(&self) -> &Self::Target {
        &self.settings
    }
}

static SETTINGS_STORE: OnceLock<Arc<SettingsStore>> = OnceLock::new();

thread_local! {
    static SCOPED_SETTINGS_STORE: RefCell<Option<Arc<SettingsStore>>> =
        const { RefCell::new(None) };
}

fn settings_store() -> Arc<SettingsStore> {
    if let Some(store) = SCOPED_SETTINGS_STORE.with(|scoped| scoped.borrow().clone()) {
        return store;
    }
    SETTINGS_STORE
        .get_or_init(|| Arc::new(SettingsStore::load()))
        .clone()
}

/// 在当前线程注入独立的设置缓存（从当前 [`crate::paths::current`] 的 settings.json 加载），
/// guard 释放时恢复之前的缓存。仅用于测试隔离：配合 [`crate::paths::scoped`] 使用，
/// 避免并行测试共享进程级设置缓存。
pub fn scoped_settings_store() -> ScopedSettingsStore {
    let store = Arc::new(SettingsStore::load());
    let previous = SCOPED_SETTINGS_STORE.with(|scoped| scoped.borrow_mut().replace(store));
    ScopedSettingsStore {
        previous,
        _not_send: PhantomData,
    }
}

/// [`scoped_settings_store`] 返回的 guard
#[must_use = "dropping the guard immediately restores the previous settings store"]
pub struct ScopedSettingsStore {
    previous: Option<Arc<SettingsStore>>,
    _not_send: PhantomData<*const ()>,
}

impl Drop for ScopedSettingsStore {
    fn drop(&mut self) {
        let previous = self.previous.take();
        SCOPED_SETTINGS_STORE.with(|scoped| *scoped.borrow_mut() = previous);
    }
}

/// 当前线程所用设置缓存的句柄，用于在测试中把同一份缓存注入其他线程，
/// 以覆盖跨线程并发写入。
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct SharedSettingsStore(Arc<SettingsStore>);

#[cfg(test)]
impl SharedSettingsStore {
    pub(crate) fn current() -> Self {
        Self(settings_store())
    }

    /// 在当前线程注入该缓存，guard 释放时恢复之前的缓存
    pub(crate) fn install(&self) -> ScopedSettingsStore {
        let previous =
            SCOPED_SETTINGS_STORE.with(|scoped| scoped.borrow_mut().replace(self.0.clone()));
        ScopedSettingsStore {
            previous,
            _not_send: PhantomData,
        }
    }
}

fn resolve_override_path(raw: &str) -> PathBuf {
    let paths = crate::paths::current();
    if raw == "~" {
        if let Some(home) = paths.user_home() {
            return home.to_path_buf();
        }
    } else if let Some(stripped) = raw.strip_prefix("~/") {
        if let Some(home) = paths.user_home() {
            return home.join(stripped);
        }
    } else if let Some(stripped) = raw.strip_prefix("~\\") {
        if let Some(home) = paths.user_home() {
            return home.join(stripped);
        }
    }

    PathBuf::from(raw)
}

pub fn get_settings() -> AppSettings {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .clone()
}

pub fn get_settings_for_frontend() -> AppSettings {
    let mut settings = get_settings();
    if let Some(sync) = &mut settings.webdav_sync {
        sync.password.clear();
    }
    if let Some(s3) = &mut settings.s3_sync {
        s3.secret_access_key.clear();
    }
    settings.webdav_backup = None;
    settings
}

/// 整体替换设置。写锁覆盖"写文件 + 替换内存"，与其他写入方串行，
/// 保证文件与内存缓存不会因并发写入而不一致。
pub fn update_settings(new_settings: AppSettings) -> Result<(), AppError> {
    update_settings_with(|_| new_settings).map(|_| ())
}

/// 在设置写锁内基于当前值构造新设置：`build` 读到的是锁内的最新值，
/// 构造结果经路径归一化后写入文件并替换内存缓存，返回替换前的旧值。
/// 用于需要与现有值合并的整体保存（如前端保存设置），避免"锁外快照 →
/// 整体替换"覆盖掉期间 [`mutate_settings`] 提交的迁移标记、同步状态等。
pub fn update_settings_with<F>(build: F) -> Result<AppSettings, AppError>
where
    F: FnOnce(&AppSettings) -> AppSettings,
{
    let store = settings_store();
    let mut guard = store.write().unwrap_or_else(|e| {
        log::warn!("设置锁已毒化，使用恢复值: {e}");
        e.into_inner()
    });
    let mut next = build(&guard);
    next.normalize_paths();
    save_settings_file(&store, &next)?;
    Ok(std::mem::replace(&mut *guard, next))
}

fn mutate_settings<F>(mutator: F) -> Result<(), AppError>
where
    F: FnOnce(&mut AppSettings),
{
    let store = settings_store();
    let mut guard = store.write().unwrap_or_else(|e| {
        log::warn!("设置锁已毒化，使用恢复值: {e}");
        e.into_inner()
    });
    let mut next = guard.clone();
    mutator(&mut next);
    next.normalize_paths();
    save_settings_file(&store, &next)?;
    *guard = next;
    Ok(())
}

pub fn is_codex_third_party_history_provider_bucket_migrated() -> bool {
    get_settings()
        .local_migrations
        .as_ref()
        .and_then(|migrations| {
            migrations
                .codex_third_party_history_provider_bucket_v1
                .as_ref()
        })
        .is_some_and(|m| m.scanned_history_files)
}

pub fn mark_codex_third_party_history_provider_bucket_migrated(
    migration: CodexThirdPartyHistoryProviderBucketMigration,
) -> Result<(), AppError> {
    mutate_settings(|settings| {
        let migrations = settings
            .local_migrations
            .get_or_insert_with(Default::default);
        migrations.codex_third_party_history_provider_bucket_v1 = Some(migration);
    })
}

pub fn is_codex_provider_template_migrated() -> bool {
    get_settings()
        .local_migrations
        .as_ref()
        .and_then(|migrations| migrations.codex_provider_template_v1.as_ref())
        .is_some()
}

pub fn mark_codex_provider_template_migrated(
    migration: CodexProviderTemplateMigration,
) -> Result<(), AppError> {
    mutate_settings(|settings| {
        let migrations = settings
            .local_migrations
            .get_or_insert_with(Default::default);
        migrations.codex_provider_template_v1 = Some(migration);
    })
}

/// 统一会话迁移标记是否覆盖指定目录。标记里没记目录（不应出现的旧格式）
/// 视为不匹配——重跑迁移是幂等的，宁可重迁也不漏迁。
pub fn is_codex_official_history_unify_migrated_for_dir(codex_dir: &str) -> bool {
    get_settings()
        .local_migrations
        .as_ref()
        .and_then(|migrations| migrations.codex_official_history_unify_v1.as_ref())
        .is_some_and(|migration| migration.codex_config_dir.as_deref() == Some(codex_dir))
}

/// 条件写入迁移完成标记：仅当此刻开关仍开启且迁移意愿仍在时才写。
/// 检查与写入在 settings 写锁内原子完成，与关闭开关路径
/// （`update_settings` / 清标记）串行，消除"迁移线程复查开关后、写标记前
/// 用户恰好关闭开关"的竞态窗口。返回是否实际写入。
pub fn mark_codex_official_history_unify_migrated_if_enabled(
    migration: CodexOfficialHistoryUnifyMigration,
) -> Result<bool, AppError> {
    let mut written = false;
    mutate_settings(|settings| {
        if settings.unify_codex_session_history
            && settings.unify_codex_migrate_existing.unwrap_or(false)
        {
            settings
                .local_migrations
                .get_or_insert_with(Default::default)
                .codex_official_history_unify_v1 = Some(migration);
            written = true;
        }
    })?;
    Ok(written)
}

pub fn clear_codex_official_history_unify_migration() -> Result<(), AppError> {
    mutate_settings(|settings| {
        if let Some(migrations) = settings.local_migrations.as_mut() {
            migrations.codex_official_history_unify_v1 = None;
        }
    })
}

/// 用户删除过的内置模型定价 ID（seed 时跳过）
pub fn deleted_builtin_model_pricing_ids() -> Vec<String> {
    get_settings()
        .local_migrations
        .map(|migrations| migrations.deleted_builtin_model_pricing)
        .unwrap_or_default()
}

/// 记录或清除"内置模型定价已被用户删除"的标记。状态未变化时不写文件。
pub fn set_builtin_model_pricing_deleted(model_id: &str, deleted: bool) -> Result<(), AppError> {
    let already = deleted_builtin_model_pricing_ids()
        .iter()
        .any(|id| id == model_id);
    if already == deleted {
        return Ok(());
    }
    mutate_settings(|settings| {
        let migrations = settings
            .local_migrations
            .get_or_insert_with(Default::default);
        let ids = &mut migrations.deleted_builtin_model_pricing;
        ids.retain(|id| id != model_id);
        if deleted {
            ids.push(model_id.to_string());
        }
    })
}

pub fn unify_codex_migrate_existing_requested() -> bool {
    get_settings().unify_codex_migrate_existing.unwrap_or(false)
}

pub fn clear_codex_unify_migrate_existing() -> Result<(), AppError> {
    mutate_settings(|settings| {
        settings.unify_codex_migrate_existing = None;
    })
}

/// 从文件重新加载设置到内存缓存
/// 用于导入配置等场景，确保内存缓存与文件同步
pub fn reload_settings() -> Result<(), AppError> {
    let (fresh_settings, load_failure, secrets_pending) = AppSettings::load_from_file();
    let store = settings_store();
    store.set_load_failure(load_failure);
    store.set_secrets_pending(secrets_pending);
    let mut guard = store.write().unwrap_or_else(|e| {
        log::warn!("设置锁已毒化，使用恢复值: {e}");
        e.into_inner()
    });
    *guard = fresh_settings;
    Ok(())
}

/// 启动时把 settings.json 中的明文 WebDAV/S3 凭据迁入凭据存储并清空对应字段
/// （降级模式下反向写回明文）。无需迁移时不写文件；返回是否重写了文件。
pub fn migrate_secrets() -> Result<bool, AppError> {
    let store = settings_store();
    let mut guard = store.write().unwrap_or_else(|e| {
        log::warn!("设置锁已毒化，使用恢复值: {e}");
        e.into_inner()
    });
    if !store.secrets_pending() {
        return Ok(false);
    }
    let next = guard.clone();
    save_settings_file(&store, &next)?;
    *guard = next;
    Ok(true)
}

/// 启动（或最近一次重新加载）时 settings.json 的加载失败记录，供前端提示用户
pub fn get_settings_load_failure() -> Option<SettingsLoadFailure> {
    settings_store().load_failure()
}

/// 用户确认后清除加载失败记录：之后的保存将以当前（默认）设置覆盖 settings.json
pub fn acknowledge_settings_load_failure() {
    settings_store().set_load_failure(None);
}

/// 返回用户在 settings.json 中为该应用配置的配置目录覆盖（已解析 `~`）。
///
/// 没有覆盖、或应用不支持目录覆盖（Claude Desktop）时返回 `None`。
pub fn override_dir(app: &AppType) -> Option<PathBuf> {
    if matches!(app, AppType::ClaudeDesktop) {
        return None;
    }
    let store = settings_store();
    let settings = store.read().ok()?;
    settings.config_dir(app).map(resolve_override_path)
}

pub fn preserve_codex_official_auth_on_switch() -> bool {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .preserve_codex_official_auth_on_switch
}

pub fn unify_codex_session_history() -> bool {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .unify_codex_session_history
}

/// Whether sticky session routing is on (see [`AppSettings::sticky_session_routing`]).
pub fn sticky_session_routing_enabled() -> bool {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .sticky_session_routing
}

/// The proxy protocol conversion engine (see [`AppSettings::codec_engine`]),
/// read on the request path by the bridges migrated to codecs.
pub fn codec_engine() -> CodecEngine {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .codec_engine
}

// ===== 当前供应商管理函数 =====

/// 获取指定应用类型的当前供应商 ID（从本地 settings 读取）
///
/// 这是设备级别的设置，不随数据库同步。
/// 如果本地没有设置，调用者应该 fallback 到数据库的 `is_current` 字段。
pub fn get_current_provider(app_type: &AppType) -> Option<String> {
    let store = settings_store();
    let settings = store.read().ok()?;
    settings.current_provider_id(app_type).map(str::to_string)
}

/// 设置指定应用类型的当前供应商 ID（保存到本地 settings）
///
/// 这是设备级别的设置，不随数据库同步。
/// 传入 `None` 会清除当前供应商设置。
pub fn set_current_provider(app_type: &AppType, id: Option<&str>) -> Result<(), AppError> {
    mutate_settings(|settings| {
        settings.set_current_provider_id(app_type, id.map(str::to_string));
    })
}

/// 获取有效的当前供应商 ID（验证存在性）
///
/// 逻辑：
/// 1. 从本地 settings 读取当前供应商 ID
/// 2. 验证该 ID 在数据库中存在
/// 3. 如果不存在则清理本地 settings，fallback 到数据库的 is_current
///
/// 这确保了返回的 ID 一定是有效的（在数据库中存在）。
/// 多设备云同步场景下，配置导入后本地 ID 可能失效，此函数会自动修复。
pub fn get_effective_current_provider(
    db: &crate::database::Database,
    app_type: &AppType,
) -> Result<Option<String>, AppError> {
    // 1. 从本地 settings 读取
    if let Some(local_id) = get_current_provider(app_type) {
        // 2. 验证该 ID 在数据库中存在
        let providers = db.get_all_providers(app_type.as_str())?;
        if providers.contains_key(&local_id) {
            // 存在，直接返回
            return Ok(Some(local_id));
        }

        // 3. 不存在，清理本地 settings
        log::warn!(
            "本地 settings 中的供应商 {} ({}) 在数据库中不存在，将清理并 fallback 到数据库",
            local_id,
            app_type.as_str()
        );
        let _ = set_current_provider(app_type, None);
    }

    // Fallback 到数据库的 is_current
    db.get_current_provider(app_type.as_str())
}

// ===== Skill 同步方式管理函数 =====

/// 获取 Skill 同步方式配置
pub fn get_skill_sync_method() -> SyncMethod {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .skill_sync_method
}

// ===== Skill 存储位置管理函数 =====

/// 获取 Skill 存储位置配置
pub fn get_skill_storage_location() -> SkillStorageLocation {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .skill_storage_location
}

/// 设置 Skill 存储位置
pub fn set_skill_storage_location(location: SkillStorageLocation) -> Result<(), AppError> {
    mutate_settings(|s| {
        s.skill_storage_location = location;
    })
}

// ===== 备份策略管理函数 =====

/// Get the effective auto-backup interval in hours (default 24)
pub fn effective_backup_interval_hours() -> u32 {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .backup_interval_hours
        .unwrap_or(24)
}

/// Get the effective backup retain count (default 10, minimum 1)
pub fn effective_backup_retain_count() -> usize {
    effective_retention_policy().backups_keep
}

// ===== 数据保留策略 =====

/// 数据保留策略：由设置项解析出的生效值，供 [`crate::database::Maintenance::run`] 使用
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// `proxy_request_logs` 明细保留天数，更早的明细汇总进 `usage_daily_rollups` 后删除（≥ 1）
    pub detail_days: u32,
    /// `usage_daily_rollups` 保留天数；`None` 表示永久保留。不会小于 `detail_days`，
    /// 否则刚汇总的日期会被立即删除
    pub rollup_days: Option<u32>,
    /// 保留的数据库备份文件数（≥ 1）
    pub backups_keep: usize,
}

impl RetentionPolicy {
    pub const DEFAULT_DETAIL_DAYS: u32 = 30;
    pub const DEFAULT_BACKUPS_KEEP: usize = 10;

    /// 从设置解析生效的保留策略（缺省值、下限与 `rollup_days ≥ detail_days` 约束）
    pub fn from_settings(settings: &AppSettings) -> Self {
        let detail_days = settings
            .usage_detail_retention_days
            .unwrap_or(Self::DEFAULT_DETAIL_DAYS)
            .max(1);
        let rollup_days = settings
            .usage_rollup_retention_days
            .filter(|days| *days > 0)
            .map(|days| days.max(detail_days));
        let backups_keep = settings
            .backup_retain_count
            .map(|n| (n as usize).max(1))
            .unwrap_or(Self::DEFAULT_BACKUPS_KEEP);
        Self {
            detail_days,
            rollup_days,
            backups_keep,
        }
    }
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self::from_settings(&AppSettings::default())
    }
}

/// 当前设置下生效的数据保留策略
pub fn effective_retention_policy() -> RetentionPolicy {
    let store = settings_store();
    let settings = store.read().unwrap_or_else(|e| {
        log::warn!("设置锁已毒化，使用恢复值: {e}");
        e.into_inner()
    });
    RetentionPolicy::from_settings(&settings)
}

// ===== 终端设置管理函数 =====

/// 获取首选终端应用
pub fn get_preferred_terminal() -> Option<String> {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .preferred_terminal
        .clone()
}

// ===== WebDAV 同步设置管理函数 =====

/// 获取 WebDAV 同步设置
pub fn get_webdav_sync_settings() -> Option<WebDavSyncSettings> {
    settings_store().read().ok()?.webdav_sync.clone()
}

/// 保存 WebDAV 同步设置
pub fn set_webdav_sync_settings(settings: Option<WebDavSyncSettings>) -> Result<(), AppError> {
    mutate_settings(|current| {
        current.webdav_sync = settings;
    })
}

/// 仅更新 WebDAV 同步状态，避免覆写 credentials/root/profile 等字段
pub fn update_webdav_sync_status(status: WebDavSyncStatus) -> Result<(), AppError> {
    mutate_settings(|current| {
        if let Some(sync) = current.webdav_sync.as_mut() {
            sync.status = status;
        }
    })
}

// ===== 同步快照密钥 =====

/// 读取 WebDAV/S3 快照签名密钥（PBKDF2 派生值，保存在凭据存储中）；未配置时返回 `None`
pub(crate) fn get_sync_snapshot_key() -> Result<Option<SyncKey>, AppError> {
    let Some(path) = AppSettings::settings_path() else {
        return Ok(None);
    };
    let policy = settings_secret_policy(&path);
    let Some(stored) = policy
        .store()
        .get(secret_store::SYNC_SNAPSHOT_KEY)?
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    SyncKey::from_hex(&stored).map(Some).ok_or_else(|| {
        AppError::localized(
            "sync.key_unreadable",
            "凭据存储中的同步密钥已损坏，请重新输入",
            "The stored sync key is corrupted. Enter it again.",
        )
    })
}

/// 设置或清除快照签名密钥，返回新密钥的指纹。
/// 清除时同时复位 WebDAV/S3 的 `signed_sync`，即用户主动退出签名校验。
pub(crate) fn set_sync_snapshot_key(passphrase: Option<&str>) -> Result<Option<String>, AppError> {
    let path = AppSettings::settings_path()
        .ok_or_else(|| AppError::Config("无法获取用户主目录".to_string()))?;
    let policy = settings_secret_policy(&path);
    match passphrase.filter(|value| !value.is_empty()) {
        Some(passphrase) => {
            let key = SyncKey::from_passphrase(passphrase)?;
            policy
                .store()
                .set(secret_store::SYNC_SNAPSHOT_KEY, &key.to_hex())?;
            Ok(Some(key.key_id()))
        }
        None => {
            policy.store().delete(secret_store::SYNC_SNAPSHOT_KEY)?;
            mutate_settings(|current| {
                if let Some(sync) = current.webdav_sync.as_mut() {
                    sync.status.signed_sync = false;
                }
                if let Some(s3) = current.s3_sync.as_mut() {
                    s3.status.signed_sync = false;
                }
            })?;
            Ok(None)
        }
    }
}

// ===== S3 同步设置管理函数 =====

pub fn get_s3_sync_settings() -> Option<S3SyncSettings> {
    settings_store().read().ok()?.s3_sync.clone()
}

pub fn set_s3_sync_settings(settings: Option<S3SyncSettings>) -> Result<(), AppError> {
    mutate_settings(|current| {
        current.s3_sync = settings;
    })
}

pub fn update_s3_sync_status(status: WebDavSyncStatus) -> Result<(), AppError> {
    mutate_settings(|current| {
        if let Some(s3) = current.s3_sync.as_mut() {
            s3.status = status;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;

    #[test]
    fn retention_policy_defaults_and_bounds() {
        assert_eq!(
            RetentionPolicy::default(),
            RetentionPolicy {
                detail_days: 30,
                rollup_days: None,
                backups_keep: 10,
            }
        );

        let settings: AppSettings = serde_json::from_value(serde_json::json!({
            "usageDetailRetentionDays": 0,
            "usageRollupRetentionDays": 0,
            "backupRetainCount": 0
        }))
        .expect("settings");
        assert_eq!(
            RetentionPolicy::from_settings(&settings),
            RetentionPolicy {
                detail_days: 1,
                rollup_days: None,
                backups_keep: 1,
            },
            "zero detail/backups clamp to 1, zero rollup days keeps rollups forever"
        );

        let settings = AppSettings {
            usage_detail_retention_days: Some(60),
            usage_rollup_retention_days: Some(14),
            backup_retain_count: Some(3),
            ..AppSettings::default()
        };
        assert_eq!(
            RetentionPolicy::from_settings(&settings),
            RetentionPolicy {
                detail_days: 60,
                rollup_days: Some(60),
                backups_keep: 3,
            },
            "rollups are never pruned before their detail rows"
        );
    }

    #[test]
    fn visible_apps_old_settings_default_claude_desktop_visible() {
        let visible: VisibleApps = serde_json::from_value(serde_json::json!({
            "claude": true,
            "codex": true,
            "gemini": true,
            "opencode": true,
            "openclaw": true,
            "hermes": true
        }))
        .expect("visible apps");

        assert!(visible.is_visible(&AppType::ClaudeDesktop));
    }

    #[test]
    fn visible_apps_accepts_claude_desktop_aliases() {
        let visible: VisibleApps = serde_json::from_value(serde_json::json!({
            "claude": true,
            "claudeDesktop": false,
            "codex": true,
            "gemini": true,
            "opencode": true,
            "openclaw": true,
            "hermes": true
        }))
        .expect("visible apps");

        assert!(!visible.is_visible(&AppType::ClaudeDesktop));
    }

    /// 旧版（双写引入前）`AppSettings` 中按应用分槽字段的冻结副本，用于验证降级兼容：
    /// 新版本写出的 settings.json 由旧结构体读取时仍能找到当前供应商、目录覆盖与可见应用。
    /// 旧结构体没有 `deny_unknown_fields`，其余字段与本测试无关，故只保留这些字段。
    /// 不要随新版本修改此副本。
    mod legacy_v1 {
        use serde::Deserialize;

        fn default_true() -> bool {
            true
        }

        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct VisibleApps {
            #[serde(default = "default_true")]
            pub claude: bool,
            #[serde(
                rename = "claude-desktop",
                alias = "claudeDesktop",
                alias = "claude_desktop",
                default = "default_true"
            )]
            pub claude_desktop: bool,
            #[serde(default = "default_true")]
            pub codex: bool,
            #[serde(default = "default_true")]
            pub gemini: bool,
            #[serde(default = "default_true")]
            pub grokbuild: bool,
            #[serde(default = "default_true")]
            pub opencode: bool,
            #[serde(default = "default_true")]
            pub openclaw: bool,
            #[serde(default)]
            pub hermes: bool,
        }

        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct AppSettings {
            #[serde(default)]
            pub visible_apps: Option<VisibleApps>,
            #[serde(default)]
            pub claude_config_dir: Option<String>,
            #[serde(default)]
            pub codex_config_dir: Option<String>,
            #[serde(default)]
            pub gemini_config_dir: Option<String>,
            #[serde(default)]
            pub grok_config_dir: Option<String>,
            #[serde(default)]
            pub opencode_config_dir: Option<String>,
            #[serde(default)]
            pub openclaw_config_dir: Option<String>,
            #[serde(default)]
            pub hermes_config_dir: Option<String>,
            #[serde(default)]
            pub current_provider_claude: Option<String>,
            #[serde(default)]
            pub current_provider_claude_desktop: Option<String>,
            #[serde(default)]
            pub current_provider_codex: Option<String>,
            #[serde(default)]
            pub current_provider_gemini: Option<String>,
            #[serde(default)]
            pub current_provider_grokbuild: Option<String>,
            #[serde(default)]
            pub current_provider_opencode: Option<String>,
            #[serde(default)]
            pub current_provider_openclaw: Option<String>,
            #[serde(default)]
            pub current_provider_hermes: Option<String>,
        }
    }

    fn settings_with_every_app() -> AppSettings {
        let mut settings = AppSettings::default();
        let mut visible = VisibleApps::default();
        for app in AppType::all() {
            let slug = app.as_str();
            settings.set_current_provider_id(&app, Some(format!("p-{slug}")));
            if !matches!(app, AppType::ClaudeDesktop) {
                settings.set_config_dir(&app, Some(format!("/cfg/{slug}")));
            }
            visible.set_visible(&app, matches!(app, AppType::Hermes));
        }
        settings.visible_apps = Some(visible);
        settings
    }

    #[test]
    fn new_settings_json_is_readable_by_the_old_struct() {
        let json = serde_json::to_string(&settings_with_every_app()).expect("serialize");
        let old: legacy_v1::AppSettings = serde_json::from_str(&json).expect("old struct parses");

        assert_eq!(old.current_provider_claude.as_deref(), Some("p-claude"));
        assert_eq!(
            old.current_provider_claude_desktop.as_deref(),
            Some("p-claude-desktop")
        );
        assert_eq!(old.current_provider_codex.as_deref(), Some("p-codex"));
        assert_eq!(old.current_provider_gemini.as_deref(), Some("p-gemini"));
        assert_eq!(
            old.current_provider_grokbuild.as_deref(),
            Some("p-grokbuild")
        );
        assert_eq!(old.current_provider_opencode.as_deref(), Some("p-opencode"));
        assert_eq!(old.current_provider_openclaw.as_deref(), Some("p-openclaw"));
        assert_eq!(old.current_provider_hermes.as_deref(), Some("p-hermes"));

        assert_eq!(old.claude_config_dir.as_deref(), Some("/cfg/claude"));
        assert_eq!(old.codex_config_dir.as_deref(), Some("/cfg/codex"));
        assert_eq!(old.gemini_config_dir.as_deref(), Some("/cfg/gemini"));
        assert_eq!(old.grok_config_dir.as_deref(), Some("/cfg/grokbuild"));
        assert_eq!(old.opencode_config_dir.as_deref(), Some("/cfg/opencode"));
        assert_eq!(old.openclaw_config_dir.as_deref(), Some("/cfg/openclaw"));
        assert_eq!(old.hermes_config_dir.as_deref(), Some("/cfg/hermes"));

        let visible = old.visible_apps.expect("visible apps");
        assert!(!visible.claude && !visible.claude_desktop && !visible.codex);
        assert!(!visible.gemini && !visible.grokbuild && !visible.opencode);
        assert!(!visible.openclaw && visible.hermes);
    }

    #[test]
    fn settings_serialize_keyed_maps_and_roundtrip() {
        let settings = settings_with_every_app();
        let value = serde_json::to_value(&settings).expect("serialize");
        assert_eq!(
            value["currentProviders"]["claude-desktop"],
            "p-claude-desktop"
        );
        assert_eq!(value["configDirs"]["grokbuild"], "/cfg/grokbuild");
        assert_eq!(value["visibleApps"]["claude-desktop"], false);

        let back: AppSettings = serde_json::from_value(value).expect("deserialize");
        assert_eq!(back.per_app, settings.per_app);
        assert_eq!(back.visible_apps, settings.visible_apps);
        for app in AppType::all() {
            assert_eq!(
                back.current_provider_id(&app),
                Some(format!("p-{}", app.as_str()).as_str())
            );
        }
    }

    #[test]
    fn old_flat_settings_json_still_loads() {
        let settings: AppSettings = serde_json::from_value(serde_json::json!({
            "currentProviderClaude": "c1",
            "currentProviderClaudeDesktop": "d1",
            "currentProviderGrokbuild": "g1",
            "claudeConfigDir": "/old/claude",
            "grokConfigDir": "/old/grok",
            "visibleApps": { "claude": true, "claudeDesktop": false, "hermes": true }
        }))
        .expect("old settings");

        assert_eq!(settings.current_provider_id(&AppType::Claude), Some("c1"));
        assert_eq!(
            settings.current_provider_id(&AppType::ClaudeDesktop),
            Some("d1")
        );
        assert_eq!(
            settings.current_provider_id(&AppType::GrokBuild),
            Some("g1")
        );
        assert_eq!(settings.current_provider_id(&AppType::Codex), None);
        assert_eq!(settings.config_dir(&AppType::Claude), Some("/old/claude"));
        assert_eq!(settings.config_dir(&AppType::GrokBuild), Some("/old/grok"));
        let visible = settings.visible_apps.expect("visible apps");
        assert!(!visible.is_visible(&AppType::ClaudeDesktop));
        assert!(visible.is_visible(&AppType::Hermes));
        assert!(
            visible.is_visible(&AppType::Codex),
            "missing key uses default"
        );
    }

    #[test]
    fn keyed_map_wins_over_flat_keys_and_keeps_unknown_apps() {
        let settings: AppSettings = serde_json::from_value(serde_json::json!({
            "currentProviders": { "claude": "from-map", "claude_desktop": "desk", "ninth": "n1" },
            "currentProviderClaude": "from-flat",
            "currentProviderCodex": "codex-flat",
            "configDirs": { "ninth": "/ninth" },
            "visibleApps": { "ninth": false }
        }))
        .expect("settings");

        assert_eq!(
            settings.current_provider_id(&AppType::Claude),
            Some("from-map")
        );
        assert_eq!(
            settings.current_provider_id(&AppType::ClaudeDesktop),
            Some("desk"),
            "map keys are normalized through registry aliases"
        );
        assert_eq!(
            settings.current_provider_id(&AppType::Codex),
            Some("codex-flat"),
            "apps missing from the map fall back to the flat key"
        );

        // 未注册应用（更新版本新增）的槽位原样保留，降级再升级不丢失
        let value = serde_json::to_value(&settings).expect("serialize");
        assert_eq!(value["currentProviders"]["ninth"], "n1");
        assert_eq!(value["configDirs"]["ninth"], "/ninth");
        assert_eq!(value["visibleApps"]["ninth"], false);
        assert!(value.get("currentProviderNinth").is_none());
    }

    #[test]
    fn frontend_save_uses_flat_keys_for_legacy_apps() {
        // 前端回传读取时的映射旧值，但按扁平键编辑：改 Codex 目录、清空 Claude 目录
        let mut value = serde_json::to_value(settings_with_every_app()).expect("serialize");
        let obj = value.as_object_mut().unwrap();
        obj.insert("codexConfigDir".into(), serde_json::json!("/edited/codex"));
        obj.remove("claudeConfigDir");
        obj.get_mut("configDirs")
            .and_then(|m| m.as_object_mut())
            .unwrap()
            .insert("ninth".into(), serde_json::json!("/ninth"));

        let saved = AppSettings::from_frontend_value(value).expect("frontend settings");
        assert_eq!(saved.config_dir(&AppType::Codex), Some("/edited/codex"));
        assert_eq!(
            saved.config_dir(&AppType::Claude),
            None,
            "cleared by the form"
        );
        assert_eq!(saved.config_dir(&AppType::Gemini), Some("/cfg/gemini"));
        assert_eq!(
            saved.per_app.config_dirs.get("ninth").map(String::as_str),
            Some("/ninth"),
            "apps without a legacy flat key keep their map entry"
        );
    }

    #[test]
    fn normalize_trims_and_drops_empty_config_dirs() {
        let mut settings = AppSettings::default()
            .with_config_dir(&AppType::Claude, "  /x  ")
            .with_config_dir(&AppType::Codex, "   ");
        settings.normalize_paths();
        assert_eq!(settings.config_dir(&AppType::Claude), Some("/x"));
        assert_eq!(settings.config_dir(&AppType::Codex), None);
    }

    mod persistence {
        use super::super::*;
        use crate::paths::{self, Paths};

        struct Env {
            _settings: ScopedSettingsStore,
            _paths: paths::ScopedPaths,
            _temp: tempfile::TempDir,
            path: PathBuf,
        }

        /// 隔离 home 目录，可选预先写入 settings.json，再加载线程内设置缓存
        fn env_with(initial: Option<&[u8]>) -> Env {
            let temp = tempfile::tempdir().expect("tempdir");
            let paths_guard = paths::scoped(Paths::with_home(temp.path()));
            let path = paths::current().settings_path();
            if let Some(bytes) = initial {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                crate::config::atomic_write(&path, bytes).unwrap();
            }
            Env {
                _settings: scoped_settings_store(),
                _paths: paths_guard,
                _temp: temp,
                path,
            }
        }

        fn dir_entries(dir: &Path) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
                .collect();
            names.sort();
            names
        }

        #[cfg(unix)]
        fn mode(path: &Path) -> u32 {
            use std::os::unix::fs::PermissionsExt;
            fs::metadata(path).unwrap().permissions().mode() & 0o777
        }

        #[test]
        fn invalid_json_is_backed_up_before_save_overwrites_it() {
            let original: &[u8] = b"{\"webdavSync\": {\"password\": \"secret\"},, broken";
            let env = env_with(Some(original));

            let failure = get_settings_load_failure().expect("load failure recorded");
            let backup = failure.backup_path.clone().expect("backup created");
            assert_eq!(fs::read(&backup).unwrap(), original);
            assert!(backup
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("settings.json.corrupt-"));

            update_settings(AppSettings::default()).expect("save allowed once backed up");

            assert_eq!(fs::read(&backup).unwrap(), original);
            let saved: AppSettings =
                serde_json::from_slice(&fs::read(&env.path).unwrap()).expect("saved valid json");
            assert!(saved.webdav_sync.is_none());
            #[cfg(unix)]
            assert_eq!(mode(&backup), 0o600);
        }

        #[test]
        fn invalid_utf8_is_treated_as_corrupt_and_backed_up() {
            let original: &[u8] = &[b'{', 0xff, 0xfe, b'}'];
            let _env = env_with(Some(original));

            let failure = get_settings_load_failure().expect("load failure recorded");
            assert_eq!(fs::read(failure.backup_path.unwrap()).unwrap(), original);
        }

        #[cfg(unix)]
        #[test]
        fn save_is_refused_when_unreadable_file_cannot_be_backed_up() {
            // 以目录占位 settings.json：读取失败（非 NotFound），无法备份
            let temp = tempfile::tempdir().expect("tempdir");
            let _paths = paths::scoped(Paths::with_home(temp.path()));
            let path = paths::current().settings_path();
            fs::create_dir_all(&path).unwrap();
            let _settings = scoped_settings_store();

            let failure = get_settings_load_failure().expect("load failure recorded");
            assert!(failure.backup_path.is_none());

            let err = update_settings(AppSettings::default()).expect_err("save refused");
            assert!(err.to_string().contains("拒绝保存"), "{err}");
            assert!(mutate_settings(|s| s.show_in_tray = false).is_err());
            assert!(path.is_dir(), "unreadable settings must not be replaced");

            acknowledge_settings_load_failure();
            assert!(get_settings_load_failure().is_none());
        }

        #[test]
        fn missing_or_valid_file_records_no_failure() {
            let _env = env_with(None);
            assert!(get_settings_load_failure().is_none());
            update_settings(AppSettings::default()).expect("save");
            reload_settings().expect("reload");
            assert!(get_settings_load_failure().is_none());
        }

        #[cfg(unix)]
        #[test]
        fn save_is_atomic_owner_only_and_leaves_no_tmp_files() {
            use std::os::unix::fs::PermissionsExt;

            let env = env_with(None);
            let dir = env.path.parent().unwrap().to_path_buf();

            update_settings(AppSettings::default()).expect("create");
            assert_eq!(mode(&env.path), 0o600);

            fs::set_permissions(&env.path, fs::Permissions::from_mode(0o644)).unwrap();
            mutate_settings(|s| s.show_in_tray = false).expect("rewrite");
            assert_eq!(mode(&env.path), 0o600);

            assert_eq!(dir_entries(&dir), vec!["settings.json".to_string()]);
        }
    }

    mod secret_store_migration {
        use super::super::*;
        use crate::paths::{self, Paths};
        use crate::secret_store::{
            scoped_policy, MemorySecretStore, ScopedSecretPolicy, SecretPolicy, SecretStore,
            S3_SECRET_ACCESS_KEY, WEBDAV_PASSWORD_KEY,
        };

        struct Env {
            _settings: ScopedSettingsStore,
            _policy: ScopedSecretPolicy,
            _paths: paths::ScopedPaths,
            _temp: tempfile::TempDir,
            path: PathBuf,
        }

        /// 隔离 home，写入 settings.json，并以 mock 凭据存储加载线程内设置缓存
        fn env_with(
            initial: serde_json::Value,
            store: Arc<MemorySecretStore>,
            inline: bool,
        ) -> Env {
            let temp = tempfile::tempdir().expect("tempdir");
            let paths_guard = paths::scoped(Paths::with_home(temp.path()));
            let path = paths::current().settings_path();
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            crate::config::atomic_write(&path, initial.to_string().as_bytes()).unwrap();
            let policy = scoped_policy(SecretPolicy::new(store, inline));
            Env {
                _settings: scoped_settings_store(),
                _policy: policy,
                _paths: paths_guard,
                _temp: temp,
                path,
            }
        }

        fn legacy_settings(password: &str, secret: &str) -> serde_json::Value {
            serde_json::json!({
                "webdavSync": {
                    "enabled": true,
                    "baseUrl": "https://dav.example.com",
                    "username": "alice",
                    "password": password
                },
                "s3Sync": {
                    "enabled": true,
                    "region": "us-east-1",
                    "bucket": "bucket",
                    "accessKeyId": "AKIA",
                    "secretAccessKey": secret
                }
            })
        }

        fn on_disk(path: &Path) -> serde_json::Value {
            serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
        }

        #[test]
        fn sync_snapshot_key_is_stored_outside_settings_and_clearing_resets_signed_sync() {
            let store = Arc::new(MemorySecretStore::new());
            let env = env_with(
                legacy_settings("dav-pass", "s3-secret"),
                store.clone(),
                false,
            );
            assert_eq!(get_sync_snapshot_key().unwrap(), None);

            let key_id = set_sync_snapshot_key(Some("correct horse battery"))
                .unwrap()
                .expect("key id");
            let key = get_sync_snapshot_key().unwrap().expect("stored key");
            assert_eq!(key.key_id(), key_id);
            assert_eq!(
                store.get(crate::secret_store::SYNC_SNAPSHOT_KEY).unwrap(),
                Some(key.to_hex()),
                "the derived key, not the passphrase, is stored"
            );
            assert!(!fs::read_to_string(&env.path)
                .unwrap()
                .contains("correct horse battery"));

            update_webdav_sync_status(WebDavSyncStatus {
                signed_sync: true,
                ..Default::default()
            })
            .unwrap();
            update_s3_sync_status(WebDavSyncStatus {
                signed_sync: true,
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                on_disk(&env.path)["webdavSync"]["status"]["signedSync"],
                true
            );

            assert_eq!(set_sync_snapshot_key(None).unwrap(), None);
            assert_eq!(get_sync_snapshot_key().unwrap(), None);
            assert!(!get_webdav_sync_settings().unwrap().status.signed_sync);
            assert!(!get_s3_sync_settings().unwrap().status.signed_sync);
        }

        #[test]
        fn corrupted_sync_snapshot_key_is_an_error_not_a_missing_key() {
            let store = Arc::new(MemorySecretStore::new());
            let _env = env_with(legacy_settings("", ""), store.clone(), false);
            store
                .set(crate::secret_store::SYNC_SNAPSHOT_KEY, "not-hex")
                .unwrap();
            assert!(get_sync_snapshot_key().is_err());
        }

        #[test]
        fn migration_moves_secrets_and_blanks_settings_fields() {
            let store = Arc::new(MemorySecretStore::new());
            let env = env_with(
                legacy_settings("dav-pass", "s3-secret"),
                store.clone(),
                false,
            );

            assert!(
                migrate_secrets().expect("migrate"),
                "plaintext must be migrated"
            );

            let disk = on_disk(&env.path);
            assert_eq!(disk["webdavSync"]["password"], "");
            assert_eq!(disk["webdavSync"]["baseUrl"], "https://dav.example.com");
            assert_eq!(disk["s3Sync"]["secretAccessKey"], "");
            assert_eq!(disk["s3Sync"]["accessKeyId"], "AKIA");
            let raw = fs::read_to_string(&env.path).unwrap();
            assert!(
                !raw.contains("dav-pass") && !raw.contains("s3-secret"),
                "{raw}"
            );

            let secrets = store.snapshot();
            assert_eq!(secrets.get(WEBDAV_PASSWORD_KEY).unwrap(), "dav-pass");
            assert_eq!(secrets.get(S3_SECRET_ACCESS_KEY).unwrap(), "s3-secret");

            // 内存中的设置保持完整，同步逻辑照常可用
            assert_eq!(get_webdav_sync_settings().unwrap().password, "dav-pass");
            assert_eq!(
                get_s3_sync_settings().unwrap().secret_access_key,
                "s3-secret"
            );

            // 已迁移后不再重写；重新加载从凭据存储补全
            assert!(!migrate_secrets().unwrap());
            reload_settings().unwrap();
            assert_eq!(get_webdav_sync_settings().unwrap().password, "dav-pass");
            assert_eq!(
                get_s3_sync_settings().unwrap().secret_access_key,
                "s3-secret"
            );
            assert!(!migrate_secrets().unwrap());
        }

        #[test]
        fn status_updates_keep_secrets_out_of_the_file() {
            let store = Arc::new(MemorySecretStore::new());
            let env = env_with(
                legacy_settings("dav-pass", "s3-secret"),
                store.clone(),
                false,
            );

            update_webdav_sync_status(WebDavSyncStatus {
                last_sync_at: Some(1),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(on_disk(&env.path)["webdavSync"]["password"], "");
            assert_eq!(store.get(WEBDAV_PASSWORD_KEY).unwrap().unwrap(), "dav-pass");
            assert!(
                !migrate_secrets().unwrap(),
                "any save completes the migration"
            );
        }

        #[test]
        fn removing_sync_config_deletes_its_secret() {
            let store = Arc::new(MemorySecretStore::new());
            let _env = env_with(
                legacy_settings("dav-pass", "s3-secret"),
                store.clone(),
                false,
            );
            migrate_secrets().unwrap();

            set_webdav_sync_settings(None).unwrap();
            assert_eq!(store.get(WEBDAV_PASSWORD_KEY).unwrap(), None);
            assert_eq!(
                store.get(S3_SECRET_ACCESS_KEY).unwrap().unwrap(),
                "s3-secret"
            );
        }

        #[test]
        fn failed_store_write_keeps_plaintext_instead_of_losing_it() {
            let store = Arc::new(MemorySecretStore::new());
            store.fail_writes(true);
            let env = env_with(
                legacy_settings("dav-pass", "s3-secret"),
                store.clone(),
                false,
            );

            migrate_secrets().expect("save still succeeds");
            let disk = on_disk(&env.path);
            assert_eq!(disk["webdavSync"]["password"], "dav-pass");
            assert_eq!(disk["s3Sync"]["secretAccessKey"], "s3-secret");
            assert!(store.snapshot().is_empty());
        }

        #[test]
        fn plaintext_mode_writes_secrets_back_for_downgrade() {
            let store = Arc::new(MemorySecretStore::new());
            store.set(WEBDAV_PASSWORD_KEY, "dav-pass").unwrap();
            store.set(S3_SECRET_ACCESS_KEY, "s3-secret").unwrap();
            let env = env_with(legacy_settings("", ""), store.clone(), true);

            assert_eq!(get_webdav_sync_settings().unwrap().password, "dav-pass");
            assert!(migrate_secrets().unwrap());
            let disk = on_disk(&env.path);
            assert_eq!(disk["webdavSync"]["password"], "dav-pass");
            assert_eq!(disk["s3Sync"]["secretAccessKey"], "s3-secret");
            assert!(
                store.snapshot().is_empty(),
                "secrets released after write-back"
            );
        }
    }
}
