//! 凭据存储：把 WebDAV/S3 密码与 Copilot/Codex OAuth 令牌移出明文 JSON。
//!
//! - [`SecretStore`] 是统一的键值接口，默认实现为系统钥匙串
//!   （macOS Keychain、Windows 凭据管理器、Linux Secret Service，见 [`KeyringSecretStore`]）；
//!   钥匙串不可用（无头 Linux、无 Secret Service 守护进程）时回退到
//!   `<应用配置目录>/secrets.json`（[`FileSecretStore`]，仅属主可读写 0600）。
//! - [`SecretPolicy`] 决定某个目录下的数据使用哪个后端，以及是否仍以明文内联写回
//!   （降级路径）。由环境变量 `CC_SWITCH_SECRET_STORE` 控制：
//!   - 未设置 / `auto` / `keychain`：优先钥匙串，不可用时回退文件；
//!   - `file`：始终使用 `secrets.json`；
//!   - `plaintext`：把已外置的凭据写回 settings.json / 令牌文件并从存储中删除，
//!     供降级到旧版本前执行一次（见 `docs/dev/SECRET_STORAGE.md`）。
//! - 测试构建或设置了 `CC_SWITCH_TEST_HOME` 时不会触碰真实钥匙串，一律使用目录内的文件后端。
//!
//! 供应商 API Key 仍保留在各 CLI 的 live 配置中（CLI 需要直接读取），不在此处管理。

use crate::error::AppError;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// 钥匙串中使用的服务名
pub const KEYCHAIN_SERVICE: &str = "cc-switch";
/// 文件回退后端的文件名（位于应用配置目录）
pub const SECRET_FILE_NAME: &str = "secrets.json";
/// 选择后端的环境变量
pub const SECRET_STORE_ENV: &str = "CC_SWITCH_SECRET_STORE";

/// WebDAV 同步密码
pub const WEBDAV_PASSWORD_KEY: &str = "settings.webdav.password";
/// S3 同步 Secret Access Key
pub const S3_SECRET_ACCESS_KEY: &str = "settings.s3.secret_access_key";
/// WebDAV/S3 快照签名密钥（PBKDF2 派生值的十六进制，两种传输共用）
pub const SYNC_SNAPSHOT_KEY: &str = "settings.sync.snapshot_key";

/// Copilot 账号 GitHub OAuth 令牌的键
pub fn copilot_token_key(account_id: &str) -> String {
    format!("copilot.github_token.{account_id}")
}

/// Codex OAuth 账号 refresh token 的键
pub fn codex_refresh_token_key(account_id: &str) -> String {
    format!("codex_oauth.refresh_token.{account_id}")
}

/// 凭据存储后端
pub trait SecretStore: Send + Sync {
    /// 后端名称（用于日志）
    fn backend(&self) -> &'static str;
    /// 读取凭据；不存在时返回 `Ok(None)`
    fn get(&self, key: &str) -> Result<Option<String>, AppError>;
    /// 写入（覆盖）凭据
    fn set(&self, key: &str, value: &str) -> Result<(), AppError>;
    /// 删除凭据；不存在时视为成功
    fn delete(&self, key: &str) -> Result<(), AppError>;
}

fn secret_error(backend: &str, action: &str, key: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Message(format!("凭据存储（{backend}）{action}失败（{key}）: {err}"))
}

// ===== 系统钥匙串 =====

/// 系统钥匙串后端。带进程内缓存：重复写入相同值、重复删除不再访问钥匙串，
/// 避免每次保存设置（如同步状态更新）都触发钥匙串调用。
pub struct KeyringSecretStore {
    service: String,
    cache: Mutex<HashMap<String, Option<String>>>,
}

impl KeyringSecretStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry, AppError> {
        keyring::Entry::new(&self.service, key)
            .map_err(|e| secret_error("keychain", "打开条目", key, e))
    }

    fn cached(&self, key: &str) -> Option<Option<String>> {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .cloned()
    }

    fn remember(&self, key: &str, value: Option<String>) {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.to_string(), value);
    }

    /// 钥匙串是否可用：后端能初始化，且读取一个不存在的条目返回 NoEntry 或成功
    fn probe(service: &str) -> Result<(), String> {
        keyring::Entry::store_status()
            .as_ref()
            .map_err(|e| e.to_string())?;
        let entry = keyring::Entry::new(service, "__probe__").map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

impl SecretStore for KeyringSecretStore {
    fn backend(&self) -> &'static str {
        "keychain"
    }

    fn get(&self, key: &str) -> Result<Option<String>, AppError> {
        if let Some(value) = self.cached(key) {
            return Ok(value);
        }
        let value = match self.entry(key)?.get_password() {
            Ok(value) => Some(value),
            Err(keyring::Error::NoEntry) => None,
            Err(e) => return Err(secret_error("keychain", "读取", key, e)),
        };
        self.remember(key, value.clone());
        Ok(value)
    }

    fn set(&self, key: &str, value: &str) -> Result<(), AppError> {
        if self.cached(key).flatten().as_deref() == Some(value) {
            return Ok(());
        }
        self.entry(key)?
            .set_password(value)
            .map_err(|e| secret_error("keychain", "写入", key, e))?;
        self.remember(key, Some(value.to_string()));
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), AppError> {
        if matches!(self.cached(key), Some(None)) {
            return Ok(());
        }
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {
                self.remember(key, None);
                Ok(())
            }
            Err(e) => Err(secret_error("keychain", "删除", key, e)),
        }
    }
}

// ===== 文件回退 =====

/// 串行化所有文件后端的读-改-写（Copilot、Codex 与设置可能写同一个文件）
static FILE_STORE_LOCK: Mutex<()> = Mutex::new(());

/// 文件回退后端：`{ "version": 1, "secrets": { key: value } }`，经
/// [`crate::config::atomic_write_secret`] 原子写入（unix 0600 / Windows 仅属主 ACL）。
pub struct FileSecretStore {
    path: PathBuf,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct SecretFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    secrets: BTreeMap<String, String>,
}

impl FileSecretStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[cfg(all(test, unix))]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Result<SecretFile, AppError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| secret_error("file", "解析", &self.path.display().to_string(), e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SecretFile::default()),
            Err(e) => Err(AppError::io(&self.path, e)),
        }
    }

    fn write(&self, file: &SecretFile) -> Result<(), AppError> {
        let json = serde_json::to_string_pretty(file)
            .map_err(|e| AppError::JsonSerialize { source: e })?;
        crate::config::atomic_write_secret(&self.path, json.as_bytes())
    }
}

impl SecretStore for FileSecretStore {
    fn backend(&self) -> &'static str {
        "file"
    }

    fn get(&self, key: &str) -> Result<Option<String>, AppError> {
        let _guard = FILE_STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        Ok(self.read()?.secrets.get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), AppError> {
        let _guard = FILE_STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut file = self.read()?;
        if file.secrets.get(key).map(String::as_str) == Some(value) {
            return Ok(());
        }
        file.version = 1;
        file.secrets.insert(key.to_string(), value.to_string());
        self.write(&file)
    }

    fn delete(&self, key: &str) -> Result<(), AppError> {
        let _guard = FILE_STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut file = self.read()?;
        if file.secrets.remove(key).is_none() {
            return Ok(());
        }
        file.version = 1;
        self.write(&file)
    }
}

// ===== 内存实现（测试替身）=====

/// 进程内存实现，用作测试中的 mock；可让写入失败以覆盖回退路径。
#[cfg(test)]
#[derive(Default)]
pub struct MemorySecretStore {
    secrets: Mutex<BTreeMap<String, String>>,
    fail_writes: std::sync::atomic::AtomicBool,
    fail_reads: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 之后的 set/delete 全部返回错误（模拟钥匙串被锁定或拒绝访问）
    pub fn fail_writes(&self, fail: bool) {
        self.fail_writes
            .store(fail, std::sync::atomic::Ordering::SeqCst);
    }

    /// 之后的 get 全部返回错误（模拟钥匙串暂时无法读取）
    pub fn fail_reads(&self, fail: bool) {
        self.fail_reads
            .store(fail, std::sync::atomic::Ordering::SeqCst);
    }

    /// 当前保存的全部凭据（按键排序）
    pub fn snapshot(&self) -> BTreeMap<String, String> {
        self.secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn check_writable(&self, action: &str, key: &str) -> Result<(), AppError> {
        if self.fail_writes.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(secret_error("memory", action, key, "write refused"));
        }
        Ok(())
    }
}

#[cfg(test)]
impl SecretStore for MemorySecretStore {
    fn backend(&self) -> &'static str {
        "memory"
    }

    fn get(&self, key: &str) -> Result<Option<String>, AppError> {
        if self.fail_reads.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(secret_error("memory", "读取", key, "read refused"));
        }
        Ok(self
            .secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), AppError> {
        self.check_writable("写入", key)?;
        self.secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), AppError> {
        self.check_writable("删除", key)?;
        self.secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(key);
        Ok(())
    }
}

// ===== 策略 =====

/// 本进程内读取失败的凭据键。读取失败时内存中的值为空，但存储中的凭据可能仍在；
/// 之后保存时不能把这个“空值”当作用户清除而删除存储中的凭据。
fn unreadable_keys() -> std::sync::MutexGuard<'static, HashSet<String>> {
    static UNREADABLE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    UNREADABLE
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// 某个目录下数据的凭据处理方式
#[derive(Clone)]
pub struct SecretPolicy {
    store: Arc<dyn SecretStore>,
    /// 为 true 时凭据以明文内联写回原文件（降级路径），并在写回成功后从存储中删除
    inline: bool,
}

impl SecretPolicy {
    pub fn new(store: Arc<dyn SecretStore>, inline: bool) -> Self {
        Self { store, inline }
    }

    pub fn store(&self) -> &Arc<dyn SecretStore> {
        &self.store
    }

    #[cfg(test)]
    pub fn is_inline(&self) -> bool {
        self.inline
    }

    /// 加载时补全凭据：内联值为空则从存储中读取。
    /// 返回 true 表示文件需要重写（外置模式下仍有明文待迁移，或内联模式下需要写回明文）。
    pub fn hydrate(&self, key: &str, inline_value: &mut String) -> bool {
        if !inline_value.is_empty() {
            return !self.inline;
        }
        match self.store.get(key) {
            Ok(value) => {
                unreadable_keys().remove(key);
                match value {
                    Some(value) if !value.is_empty() => {
                        *inline_value = value;
                        self.inline
                    }
                    _ => false,
                }
            }
            Err(e) => {
                log::warn!(
                    "读取凭据失败（{}），按未配置处理，存储中的凭据保留: {e}",
                    self.store.backend()
                );
                unreadable_keys().insert(key.to_string());
                false
            }
        }
    }

    /// 保存时外置凭据，返回应写入文件的内联值：
    /// - 外置模式：写入存储成功返回空串；写入失败时保留明文（0600 文件），不丢数据；
    ///   值为空表示已清除，同时删除存储中的旧值（加载时读取失败的键除外，避免误删）；
    /// - 内联模式：原样返回，存储中的副本在文件写成功后由 [`Self::release`] 删除。
    pub fn externalize(&self, key: &str, value: &str) -> String {
        if self.inline {
            return value.to_string();
        }
        if value.is_empty() {
            if unreadable_keys().contains(key) {
                log::warn!("凭据 {key} 加载时读取失败，保留存储中的值");
            } else if let Err(e) = self.store.delete(key) {
                log::warn!("删除凭据失败: {e}");
            }
            return String::new();
        }
        match self.store.set(key, value) {
            Ok(()) => {
                unreadable_keys().remove(key);
                String::new()
            }
            Err(e) => {
                log::warn!("写入凭据存储失败，暂以明文保留在文件中: {e}");
                value.to_string()
            }
        }
    }

    /// 文件已写成功后调用：内联模式下删除存储中的副本
    pub fn release<'a>(&self, keys: impl IntoIterator<Item = &'a str>) {
        if !self.inline {
            return;
        }
        for key in keys {
            if let Err(e) = self.store.delete(key) {
                log::warn!("删除凭据失败: {e}");
            }
        }
    }

    /// 删除不再使用的凭据（账号被移除等），失败只记录日志
    pub fn forget(&self, key: &str) {
        match self.store.delete(key) {
            Ok(()) => {
                unreadable_keys().remove(key);
            }
            Err(e) => log::warn!("删除凭据失败: {e}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Auto,
    File,
    Plaintext,
}

fn mode_from_env() -> Mode {
    match std::env::var(SECRET_STORE_ENV)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "file" => Mode::File,
        "plaintext" | "inline" => Mode::Plaintext,
        "" | "auto" | "keychain" => Mode::Auto,
        other => {
            log::warn!("未知的 {SECRET_STORE_ENV} 值 {other:?}，按 auto 处理");
            Mode::Auto
        }
    }
}

/// 测试构建或测试 HOME 下不访问真实钥匙串
fn keychain_allowed() -> bool {
    if cfg!(test) {
        return false;
    }
    std::env::var("CC_SWITCH_TEST_HOME")
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
}

/// 探测 `service` 对应的钥匙串，可用时返回其实例
fn probe_keychain(service: &str) -> Option<Arc<dyn SecretStore>> {
    match KeyringSecretStore::probe(service) {
        Ok(()) => {
            log::info!("凭据存储：使用系统钥匙串");
            Some(Arc::new(KeyringSecretStore::new(service)))
        }
        Err(e) => {
            log::warn!("系统钥匙串不可用，回退到 {SECRET_FILE_NAME}（0600）: {e}");
            None
        }
    }
}

/// 进程级的钥匙串实例（探测只做一次）；不可用时为 None
fn keychain() -> Option<Arc<dyn SecretStore>> {
    static KEYCHAIN: OnceLock<Option<Arc<dyn SecretStore>>> = OnceLock::new();
    KEYCHAIN
        .get_or_init(|| probe_keychain(KEYCHAIN_SERVICE))
        .clone()
}

#[cfg(test)]
thread_local! {
    static SCOPED_POLICY: std::cell::RefCell<Option<SecretPolicy>> = const { std::cell::RefCell::new(None) };
}

/// 返回 `dir`（应用配置目录或 settings.json 所在目录）下数据应使用的凭据策略。
pub fn policy_for_dir(dir: &Path) -> SecretPolicy {
    #[cfg(test)]
    if let Some(policy) = SCOPED_POLICY.with(|scoped| scoped.borrow().clone()) {
        return policy;
    }
    select_policy(dir, mode_from_env(), || {
        if keychain_allowed() {
            keychain()
        } else {
            None
        }
    })
}

/// 按模式选择后端：`keychain` 返回 None（不可用或不允许）时回退到 `dir` 下的 secrets.json
fn select_policy(
    dir: &Path,
    mode: Mode,
    keychain: impl FnOnce() -> Option<Arc<dyn SecretStore>>,
) -> SecretPolicy {
    let file_store =
        || -> Arc<dyn SecretStore> { Arc::new(FileSecretStore::new(dir.join(SECRET_FILE_NAME))) };
    let store = match mode {
        Mode::File => file_store(),
        Mode::Auto | Mode::Plaintext => keychain().unwrap_or_else(file_store),
    };
    SecretPolicy::new(store, mode == Mode::Plaintext)
}

/// 在当前线程注入凭据策略（测试用 mock），guard 释放时恢复。
#[cfg(test)]
pub fn scoped_policy(policy: SecretPolicy) -> ScopedSecretPolicy {
    let previous = SCOPED_POLICY.with(|scoped| scoped.borrow_mut().replace(policy));
    ScopedSecretPolicy {
        previous,
        _not_send: std::marker::PhantomData,
    }
}

/// [`scoped_policy`] 返回的 guard
#[cfg(test)]
#[must_use = "dropping the guard immediately restores the previous secret policy"]
pub struct ScopedSecretPolicy {
    previous: Option<SecretPolicy>,
    _not_send: std::marker::PhantomData<*const ()>,
}

#[cfg(test)]
impl Drop for ScopedSecretPolicy {
    fn drop(&mut self) {
        let previous = self.previous.take();
        SCOPED_POLICY.with(|scoped| *scoped.borrow_mut() = previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn file_store_round_trip_is_owner_only() {
        let temp = tempfile::tempdir().unwrap();
        let store = FileSecretStore::new(temp.path().join(SECRET_FILE_NAME));
        assert_eq!(store.get("a").unwrap(), None);
        store.set("a", "one").unwrap();
        store.set("b", "two").unwrap();
        assert_eq!(store.get("a").unwrap().as_deref(), Some("one"));
        store.delete("a").unwrap();
        store.delete("missing").unwrap();
        assert_eq!(store.get("a").unwrap(), None);
        assert_eq!(store.get("b").unwrap().as_deref(), Some("two"));
        #[cfg(unix)]
        assert_eq!(mode_of(store.path()), 0o600);
    }

    #[test]
    fn test_build_never_selects_the_keychain() {
        let temp = tempfile::tempdir().unwrap();
        let policy = policy_for_dir(temp.path());
        assert_eq!(policy.store().backend(), "file");
        assert!(!policy.is_inline());
    }

    #[test]
    fn hydrate_reads_from_store_and_flags_pending_migration() {
        let store = Arc::new(MemorySecretStore::new());
        store.set("k", "from-store").unwrap();
        let policy = SecretPolicy::new(store.clone(), false);

        let mut blank = String::new();
        assert!(!policy.hydrate("k", &mut blank));
        assert_eq!(blank, "from-store");

        let mut inline = "plain".to_string();
        assert!(
            policy.hydrate("k", &mut inline),
            "inline secret must be migrated"
        );
        assert_eq!(inline, "plain");

        let mut missing = String::new();
        assert!(!policy.hydrate("absent", &mut missing));
        assert!(missing.is_empty());
    }

    #[test]
    fn externalize_moves_secret_and_falls_back_to_plaintext_on_failure() {
        let store = Arc::new(MemorySecretStore::new());
        let policy = SecretPolicy::new(store.clone(), false);

        assert_eq!(policy.externalize("k", "s3cret"), "");
        assert_eq!(store.get("k").unwrap().as_deref(), Some("s3cret"));

        assert_eq!(policy.externalize("k", ""), "");
        assert_eq!(
            store.get("k").unwrap(),
            None,
            "cleared value deletes the secret"
        );

        store.fail_writes(true);
        assert_eq!(policy.externalize("k", "kept"), "kept");
    }

    #[test]
    fn failed_read_does_not_delete_the_stored_secret_on_save() {
        // 使用独立的键，避免与其他并行测试共享“读取失败”标记
        let key = "test.unreadable.failed_read";
        let store = Arc::new(MemorySecretStore::new());
        store.set(key, "kept").unwrap();
        let policy = SecretPolicy::new(store.clone(), false);

        store.fail_reads(true);
        let mut value = String::new();
        assert!(!policy.hydrate(key, &mut value));
        assert!(value.is_empty());
        store.fail_reads(false);

        // 保存时内存中的值为空：不能当作用户清除而删除存储中的凭据
        assert_eq!(policy.externalize(key, &value), "");
        assert_eq!(store.get(key).unwrap().as_deref(), Some("kept"));

        // 重新写入后恢复正常：之后清空会删除
        assert_eq!(policy.externalize(key, "new"), "");
        assert_eq!(policy.externalize(key, ""), "");
        assert_eq!(store.get(key).unwrap(), None);
    }

    #[test]
    fn inline_policy_writes_back_and_releases_after_write() {
        let store = Arc::new(MemorySecretStore::new());
        store.set("k", "from-store").unwrap();
        let policy = SecretPolicy::new(store.clone(), true);

        let mut value = String::new();
        assert!(
            policy.hydrate("k", &mut value),
            "inline mode rewrites the file"
        );
        assert_eq!(policy.externalize("k", &value), "from-store");
        assert_eq!(store.get("k").unwrap().as_deref(), Some("from-store"));
        policy.release(["k"]);
        assert_eq!(store.get("k").unwrap(), None);
    }

    #[test]
    fn scoped_policy_overrides_and_restores() {
        let temp = tempfile::tempdir().unwrap();
        {
            let _guard =
                scoped_policy(SecretPolicy::new(Arc::new(MemorySecretStore::new()), false));
            assert_eq!(policy_for_dir(temp.path()).store().backend(), "memory");
        }
        assert_eq!(policy_for_dir(temp.path()).store().backend(), "file");
    }

    #[test]
    fn select_policy_falls_back_to_file_when_keychain_is_unavailable() {
        let temp = tempfile::tempdir().unwrap();
        let auto = select_policy(temp.path(), Mode::Auto, || None);
        assert_eq!(auto.store().backend(), "file");
        assert!(!auto.is_inline());
        let plaintext = select_policy(temp.path(), Mode::Plaintext, || None);
        assert_eq!(plaintext.store().backend(), "file");
        assert!(plaintext.is_inline());

        let memory: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
        let chosen = select_policy(temp.path(), Mode::Auto, || Some(memory.clone()));
        assert_eq!(chosen.store().backend(), "memory");
        let file_only = select_policy(temp.path(), Mode::File, || {
            panic!("file mode must not probe the keychain")
        });
        assert_eq!(file_only.store().backend(), "file");
    }
}

/// 真实钥匙串检查（Linux Secret Service），默认忽略，由
/// `scripts/check-linux-secret-service.sh` 运行：
/// - 在 `dbus-run-session` + 已解锁的 gnome-keyring 下运行除 `probe_fails_without_dbus` 外的测试；
/// - 在没有 D-Bus 会话总线的环境中单独运行 `probe_fails_without_dbus`。
///
/// 服务名取 `CC_SWITCH_KEYRING_TEST_SERVICE`（默认 `cc-switch-test-<pid>`），不会触碰 `cc-switch` 条目。
#[cfg(all(test, target_os = "linux"))]
mod keyring_live {
    use super::*;

    fn service() -> String {
        std::env::var("CC_SWITCH_KEYRING_TEST_SERVICE")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| format!("cc-switch-test-{}", std::process::id()))
    }

    /// 绕过进程内缓存，直接查询钥匙串
    fn fresh_get(key: &str) -> Option<String> {
        KeyringSecretStore::new(service()).get(key).unwrap()
    }

    #[test]
    #[ignore = "needs a D-Bus session with an unlocked Secret Service (scripts/check-linux-secret-service.sh)"]
    fn probe_and_round_trip() {
        let service = service();
        KeyringSecretStore::probe(&service).expect("Secret Service should be reachable");

        let store = KeyringSecretStore::new(service.as_str());
        let key = "test.round_trip";
        assert_eq!(store.backend(), "keychain");
        assert_eq!(store.get(key).unwrap(), None);
        store.set(key, "one").unwrap();
        assert_eq!(store.get(key).unwrap().as_deref(), Some("one"));
        assert_eq!(fresh_get(key).as_deref(), Some("one"));
        store.set(key, "two").unwrap();
        assert_eq!(fresh_get(key).as_deref(), Some("two"));
        store.delete(key).unwrap();
        store.delete(key).unwrap();
        assert_eq!(store.get(key).unwrap(), None);
        assert_eq!(fresh_get(key), None);
    }

    #[test]
    #[ignore = "needs a D-Bus session with an unlocked Secret Service (scripts/check-linux-secret-service.sh)"]
    fn policy_externalizes_hydrates_and_forgets() {
        let store: Arc<dyn SecretStore> = Arc::new(KeyringSecretStore::new(service()));
        let policy = SecretPolicy::new(store, false);
        let copilot = copilot_token_key("acct");

        for (key, secret) in [(WEBDAV_PASSWORD_KEY, "dav-pw"), (copilot.as_str(), "gho_x")] {
            assert_eq!(policy.externalize(key, secret), "", "{key} moves out");
            assert_eq!(fresh_get(key).as_deref(), Some(secret));

            let mut blank = String::new();
            assert!(!policy.hydrate(key, &mut blank));
            assert_eq!(blank, secret);

            let mut inline = secret.to_string();
            assert!(
                policy.hydrate(key, &mut inline),
                "an inline {key} must be migrated"
            );
        }

        policy.forget(&copilot);
        assert_eq!(fresh_get(&copilot), None);

        assert_eq!(policy.externalize(WEBDAV_PASSWORD_KEY, ""), "");
        assert_eq!(fresh_get(WEBDAV_PASSWORD_KEY), None);
    }

    #[test]
    #[ignore = "needs a D-Bus session with an unlocked Secret Service (scripts/check-linux-secret-service.sh)"]
    fn plaintext_mode_writes_back_and_releases() {
        let key = copilot_token_key("downgrade");
        KeyringSecretStore::new(service())
            .set(&key, "gho_downgrade")
            .unwrap();

        let store: Arc<dyn SecretStore> = Arc::new(KeyringSecretStore::new(service()));
        let policy = SecretPolicy::new(store, true);
        let mut value = String::new();
        assert!(
            policy.hydrate(&key, &mut value),
            "plaintext mode rewrites the file"
        );
        assert_eq!(value, "gho_downgrade");
        assert_eq!(policy.externalize(&key, &value), "gho_downgrade");
        assert_eq!(fresh_get(&key).as_deref(), Some("gho_downgrade"));

        policy.release([key.as_str()]);
        assert_eq!(fresh_get(&key), None);
    }

    /// 与独立客户端互通：读取脚本用 `secret-tool store` 预置的条目，再写入新值，
    /// 由脚本用 `secret-tool lookup` 校验后清除
    #[test]
    #[ignore = "needs a D-Bus session with an unlocked Secret Service (scripts/check-linux-secret-service.sh)"]
    fn shares_entries_with_secret_tool() {
        let seed = std::env::var("CC_SWITCH_KEYRING_TEST_SEED")
            .expect("CC_SWITCH_KEYRING_TEST_SEED is set by scripts/check-linux-secret-service.sh");
        let key = "test.external";
        let store = KeyringSecretStore::new(service());
        assert_eq!(store.get(key).unwrap().as_deref(), Some(seed.as_str()));
        store.set(key, "written-by-cc-switch").unwrap();
        assert_eq!(fresh_get(key).as_deref(), Some("written-by-cc-switch"));
    }

    #[test]
    #[ignore = "needs a D-Bus session with an unlocked Secret Service (scripts/check-linux-secret-service.sh)"]
    fn cache_masks_an_out_of_band_delete() {
        let key = "test.out_of_band";
        let cached = KeyringSecretStore::new(service());
        cached.set(key, "v").unwrap();

        // 另一个实例（如 secret-tool / Seahorse）删除条目：缓存实例仍返回旧值，
        // 新实例看到条目已不存在；缓存实例的 delete 仍会成功（NoEntry 视为成功）
        KeyringSecretStore::new(service()).delete(key).unwrap();
        assert_eq!(cached.get(key).unwrap().as_deref(), Some("v"));
        assert_eq!(fresh_get(key), None);
        cached.delete(key).unwrap();
        assert_eq!(cached.get(key).unwrap(), None);
    }

    #[test]
    #[ignore = "must run without a D-Bus session bus (scripts/check-linux-secret-service.sh)"]
    fn probe_fails_without_dbus() {
        assert!(
            std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none(),
            "run with DBUS_SESSION_BUS_ADDRESS unset"
        );
        let service = service();
        assert!(KeyringSecretStore::probe(&service).is_err());

        let temp = tempfile::tempdir().unwrap();
        let policy = select_policy(temp.path(), Mode::Auto, || probe_keychain(&service));
        assert_eq!(policy.store().backend(), "file");
        assert!(!policy.is_inline());
        assert_eq!(policy.externalize(WEBDAV_PASSWORD_KEY, "dav-pw"), "");

        let path = temp.path().join(SECRET_FILE_NAME);
        assert_eq!(
            FileSecretStore::new(&path)
                .get(WEBDAV_PASSWORD_KEY)
                .unwrap()
                .as_deref(),
            Some("dav-pw")
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
