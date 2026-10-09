//! 托盘展示用的用量缓存（进程内、写穿式）。
//!
//! 各 usage 查询命令成功时写入；系统托盘构建菜单时读取。不持久化，
//! 进程重启即空，由下一次自动查询或托盘悬停触发的刷新重新填充。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::app_config::AppType;
use crate::provider::UsageResult;
use crate::services::subscription::SubscriptionQuota;

/// 持久化快照超过此时长视为过期，启动时不再载入
pub const MAX_PERSISTED_AGE_SECS: i64 = 24 * 60 * 60;
const SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubscriptionEntry {
    app_type: AppType,
    cached_at: i64,
    quota: SubscriptionQuota,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScriptEntry {
    app_type: AppType,
    provider_id: String,
    cached_at: i64,
    result: UsageResult,
}

/// 磁盘快照：`~/.cc-switch/usage-cache.json`。只含用量数字，不含密钥。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    version: u32,
    #[serde(default)]
    subscription: Vec<SubscriptionEntry>,
    #[serde(default)]
    script: Vec<ScriptEntry>,
}

#[derive(Default)]
pub struct UsageCache {
    subscription: RwLock<HashMap<AppType, SubscriptionQuota>>,
    script: RwLock<HashMap<(AppType, String), UsageResult>>,
    /// 写入时间（unix 秒），仅用于持久化快照的过期判断
    stamps: RwLock<HashMap<String, i64>>,
    /// 持久化文件；None = 仅内存（测试 / 无配置目录）
    persist_path: Option<PathBuf>,
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

fn subscription_stamp_key(app_type: &AppType) -> String {
    format!("sub:{}", app_type.as_str())
}

fn script_stamp_key(app_type: &AppType, provider_id: &str) -> String {
    format!("script:{}:{provider_id}", app_type.as_str())
}

impl UsageCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 带持久化的缓存：启动时载入 `path` 里 24 小时内的快照，之后每次写入都落盘，
    /// 让托盘在应用重启后立刻显示上次的用量，而不是等下一次查询。
    pub fn with_persistence(path: PathBuf) -> Self {
        let cache = Self {
            persist_path: Some(path.clone()),
            ..Self::default()
        };
        cache.load_from(&path, now_secs());
        cache
    }

    fn load_from(&self, path: &Path, now: i64) {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                log::warn!("[UsageCache] 读取用量快照失败: {e}");
                return;
            }
        };
        let snapshot: Snapshot = match serde_json::from_str(&raw) {
            Ok(snapshot) => snapshot,
            Err(e) => {
                log::warn!("[UsageCache] 用量快照格式无效，忽略: {e}");
                return;
            }
        };
        let fresh = |cached_at: i64| now - cached_at <= MAX_PERSISTED_AGE_SECS && cached_at <= now;
        let (Ok(mut subs), Ok(mut scripts), Ok(mut stamps)) = (
            self.subscription.write(),
            self.script.write(),
            self.stamps.write(),
        ) else {
            return;
        };
        for entry in snapshot
            .subscription
            .into_iter()
            .filter(|e| fresh(e.cached_at))
        {
            stamps.insert(subscription_stamp_key(&entry.app_type), entry.cached_at);
            subs.insert(entry.app_type, entry.quota);
        }
        for entry in snapshot.script.into_iter().filter(|e| fresh(e.cached_at)) {
            stamps.insert(
                script_stamp_key(&entry.app_type, &entry.provider_id),
                entry.cached_at,
            );
            scripts.insert((entry.app_type, entry.provider_id), entry.result);
        }
    }

    /// 把当前内容写回磁盘（best-effort；失败只记日志）
    fn persist(&self) {
        let Some(path) = &self.persist_path else {
            return;
        };
        let snapshot = {
            let (Ok(subs), Ok(scripts), Ok(stamps)) = (
                self.subscription.read(),
                self.script.read(),
                self.stamps.read(),
            ) else {
                return;
            };
            let now = now_secs();
            Snapshot {
                version: SNAPSHOT_VERSION,
                subscription: subs
                    .iter()
                    .map(|(app_type, quota)| SubscriptionEntry {
                        app_type: app_type.clone(),
                        cached_at: *stamps
                            .get(&subscription_stamp_key(app_type))
                            .unwrap_or(&now),
                        quota: quota.clone(),
                    })
                    .collect(),
                script: scripts
                    .iter()
                    .map(|((app_type, provider_id), result)| ScriptEntry {
                        app_type: app_type.clone(),
                        provider_id: provider_id.clone(),
                        cached_at: *stamps
                            .get(&script_stamp_key(app_type, provider_id))
                            .unwrap_or(&now),
                        result: result.clone(),
                    })
                    .collect(),
            }
        };
        match serde_json::to_vec_pretty(&snapshot) {
            Ok(bytes) => {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = crate::config::atomic_write(path, &bytes) {
                    log::warn!("[UsageCache] 写入用量快照失败: {e}");
                }
            }
            Err(e) => log::warn!("[UsageCache] 序列化用量快照失败: {e}"),
        }
    }

    pub fn put_subscription(&self, app_type: AppType, quota: SubscriptionQuota) {
        if let Ok(mut stamps) = self.stamps.write() {
            stamps.insert(subscription_stamp_key(&app_type), now_secs());
        }
        if let Ok(mut w) = self.subscription.write() {
            w.insert(app_type, quota);
        }
        self.persist();
    }

    pub fn put_script(&self, app_type: AppType, provider_id: String, result: UsageResult) {
        if let Ok(mut stamps) = self.stamps.write() {
            stamps.insert(script_stamp_key(&app_type, &provider_id), now_secs());
        }
        if let Ok(mut w) = self.script.write() {
            w.insert((app_type, provider_id), result);
        }
        self.persist();
    }

    /// 以借用形式暴露订阅快照，避免托盘每次重建时深拷贝整个 `SubscriptionQuota`。
    pub fn with_subscription<R>(
        &self,
        app_type: &AppType,
        f: impl FnOnce(&SubscriptionQuota) -> R,
    ) -> Option<R> {
        self.subscription
            .read()
            .ok()
            .and_then(|r| r.get(app_type).map(f))
    }

    /// 以借用形式暴露脚本型用量结果，同上。
    pub fn with_script<R>(
        &self,
        app_type: &AppType,
        provider_id: &str,
        f: impl FnOnce(&UsageResult) -> R,
    ) -> Option<R> {
        self.script
            .read()
            .ok()
            .and_then(|r| r.get(&(app_type.clone(), provider_id.to_string())).map(f))
    }

    pub fn invalidate_script(&self, app_type: &AppType, provider_id: &str) {
        // 热路径会对每个禁用脚本的 provider 在托盘重建时调用一次：先走读锁
        // `contains_key` 快速放行"本来就不在缓存里"的常见情况，避免无谓的写锁升级。
        let key = (app_type.clone(), provider_id.to_string());
        if !self.script.read().is_ok_and(|r| r.contains_key(&key)) {
            return;
        }
        if let Ok(mut w) = self.script.write() {
            w.remove(&key);
        }
        if let Ok(mut stamps) = self.stamps.write() {
            stamps.remove(&script_stamp_key(app_type, provider_id));
        }
        self.persist();
    }

    pub fn invalidate_subscription(&self, app_type: &AppType) {
        if !self
            .subscription
            .read()
            .is_ok_and(|r| r.contains_key(app_type))
        {
            return;
        }
        if let Ok(mut w) = self.subscription.write() {
            w.remove(app_type);
        }
        if let Ok(mut stamps) = self.stamps.write() {
            stamps.remove(&subscription_stamp_key(app_type));
        }
        self.persist();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::subscription::CredentialStatus;

    fn fake_quota() -> SubscriptionQuota {
        SubscriptionQuota {
            tool: "claude".to_string(),
            credential_status: CredentialStatus::Valid,
            credential_message: None,
            success: true,
            tiers: vec![],
            extra_usage: None,
            error: None,
            queried_at: Some(0),
        }
    }

    fn fake_result() -> UsageResult {
        UsageResult {
            success: true,
            data: None,
            error: None,
        }
    }

    #[test]
    fn subscription_round_trip() {
        let cache = UsageCache::new();
        assert!(cache
            .with_subscription(&AppType::Claude, |q| q.success)
            .is_none());
        cache.put_subscription(AppType::Claude, fake_quota());
        let got = cache
            .with_subscription(&AppType::Claude, |q| q.success)
            .unwrap();
        assert!(got);
        assert!(cache
            .with_subscription(&AppType::Codex, |q| q.success)
            .is_none());
    }

    #[test]
    fn script_round_trip_and_invalidate() {
        let cache = UsageCache::new();
        assert!(cache
            .with_script(&AppType::Codex, "pid", |r| r.success)
            .is_none());
        cache.put_script(AppType::Codex, "pid".to_string(), fake_result());
        assert!(cache
            .with_script(&AppType::Codex, "pid", |r| r.success)
            .is_some());
        cache.invalidate_script(&AppType::Codex, "pid");
        assert!(cache
            .with_script(&AppType::Codex, "pid", |r| r.success)
            .is_none());
    }

    #[test]
    fn script_keys_isolated_by_app_type() {
        let cache = UsageCache::new();
        cache.put_script(AppType::Claude, "same".to_string(), fake_result());
        assert!(cache
            .with_script(&AppType::Claude, "same", |r| r.success)
            .is_some());
        assert!(cache
            .with_script(&AppType::Codex, "same", |r| r.success)
            .is_none());
    }

    fn quota_with(utilization: f64) -> SubscriptionQuota {
        let mut quota = fake_quota();
        quota.tiers = vec![crate::services::subscription::QuotaTier {
            name: "five_hour".to_string(),
            utilization,
            resets_at: None,
            used_value_usd: None,
            max_value_usd: None,
        }];
        quota
    }

    fn script_with(remaining: f64) -> UsageResult {
        UsageResult {
            success: true,
            data: Some(vec![crate::provider::UsageData {
                plan_name: Some("plan".to_string()),
                remaining: Some(remaining),
                total: None,
                used: None,
                unit: Some("USD".to_string()),
                is_valid: Some(true),
                invalid_message: None,
                extra: None,
            }]),
            error: None,
        }
    }

    #[test]
    fn persists_and_reloads_fresh_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("usage-cache.json");

        let cache = UsageCache::with_persistence(path.clone());
        cache.put_subscription(AppType::Claude, quota_with(42.0));
        cache.put_script(AppType::Codex, "kimi".to_string(), script_with(7.5));
        assert!(path.exists(), "snapshot written on put");

        let reloaded = UsageCache::with_persistence(path.clone());
        assert_eq!(
            reloaded.with_subscription(&AppType::Claude, |q| q.tiers[0].utilization),
            Some(42.0)
        );
        assert_eq!(
            reloaded.with_script(&AppType::Codex, "kimi", |r| r
                .data
                .as_ref()
                .and_then(|d| d[0].remaining)),
            Some(Some(7.5))
        );

        // invalidation is persisted too
        reloaded.invalidate_script(&AppType::Codex, "kimi");
        let again = UsageCache::with_persistence(path);
        assert!(again.with_script(&AppType::Codex, "kimi", |_| ()).is_none());
        assert!(again.with_subscription(&AppType::Claude, |_| ()).is_some());
    }

    #[test]
    fn drops_stale_and_invalid_snapshots() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("usage-cache.json");
        let now = now_secs();
        let snapshot = Snapshot {
            version: SNAPSHOT_VERSION,
            subscription: vec![
                SubscriptionEntry {
                    app_type: AppType::Claude,
                    cached_at: now - MAX_PERSISTED_AGE_SECS - 1,
                    quota: quota_with(99.0),
                },
                SubscriptionEntry {
                    app_type: AppType::Codex,
                    cached_at: now - 60,
                    quota: quota_with(10.0),
                },
            ],
            script: vec![],
        };
        std::fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();

        let cache = UsageCache::with_persistence(path.clone());
        assert!(cache.with_subscription(&AppType::Claude, |_| ()).is_none());
        assert_eq!(
            cache.with_subscription(&AppType::Codex, |q| q.tiers[0].utilization),
            Some(10.0)
        );

        std::fs::write(&path, b"{ not json").unwrap();
        let broken = UsageCache::with_persistence(path);
        assert!(broken.with_subscription(&AppType::Codex, |_| ()).is_none());
    }

    #[test]
    fn memory_only_cache_never_touches_disk() {
        let cache = UsageCache::new();
        cache.put_subscription(AppType::Gemini, quota_with(1.0));
        assert!(cache.persist_path.is_none());
        assert!(cache.with_subscription(&AppType::Gemini, |_| ()).is_some());
    }
}
