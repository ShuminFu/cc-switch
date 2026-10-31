//! 累加模式应用（OpenCode / OpenClaw / Hermes）的条目级接管
//!
//! 这些应用的 live 同时承载多个供应商条目。接管只改写应用当前选中条目的基础地址
//! 与 API Key（指向 `/<app>/v1` 与本安装的代理 token），其余条目与配置不动；
//! 代理按该条目对应的 CC Switch 供应商（及故障转移队列）转发。
//!
//! 备份仍是接管前的整份 live。恢复时只把带占位符的条目的端点字段写回备份中的
//! 值，因此接管期间对其它条目的改动不会被备份覆盖；只有恢复后的 live 与备份完全
//! 一致时才再用接管快照逐字节还原（见 `restore_takeover_raw_snapshot`）。

use super::*;
use crate::app_config::AppType;
use crate::apps::adapter::{endpoint_has_proxy_token, EntryEndpoint, EntryTakeover};
use crate::apps::registry;
use crate::database::Database;
use crate::error::AppError;
use crate::proxy::inbound_auth::LEGACY_PROXY_TOKEN;
use serde_json::Value;

/// 应用的条目级接管能力（只有累加模式应用返回 `Some`）
pub(in crate::services::proxy) fn entry_takeover(
    app_type: &AppType,
) -> Option<&'static dyn EntryTakeover> {
    crate::apps::adapter(app_type)
        .takeover()
        .and_then(|takeover| takeover.entry_takeover())
}

impl ProxyService {
    /// 读取累加模式应用的整份 live 配置
    pub(in crate::services::proxy) fn read_entries_live(
        &self,
        app_type: &AppType,
    ) -> Result<Value, String> {
        self.read_live_for_takeover(app_type)
    }

    /// 应用当前使用的供应商条目：默认模型引用的条目 → 当前供应商 → 唯一条目
    fn select_takeover_entry(
        &self,
        app_type: &AppType,
        entries: &dyn EntryTakeover,
        live: &Value,
    ) -> Option<(String, Value)> {
        let mut candidates = entries.entries(live);
        let current = crate::settings::get_effective_current_provider(&self.db, app_type)
            .ok()
            .flatten();
        for id in [entries.default_entry(live), current].into_iter().flatten() {
            if let Some(index) = candidates.iter().position(|(entry_id, _)| *entry_id == id) {
                return Some(candidates.swap_remove(index));
            }
        }
        (candidates.len() == 1).then(|| candidates.remove(0))
    }

    /// 接管选中条目：改写其端点并把它设为该应用的当前供应商（代理据此转发）
    pub(in crate::services::proxy) fn takeover_entry(
        &self,
        app_type: &AppType,
        mode: TakeoverMode,
        base_url: &str,
        live: &Value,
    ) -> Result<(), TakeoverError> {
        let display_name = registry::descriptor(app_type).display_name;
        let Some(entries) = entry_takeover(app_type) else {
            return Err(TakeoverError::Unsupported(app_type.clone()));
        };
        // 严格模式（界面上的接管开关）报告原因；尽力模式（全部接管）跳过不适用的应用
        let skip_or = |error: TakeoverError| match mode {
            TakeoverMode::Strict => Err(error),
            TakeoverMode::BestEffort => {
                log::info!("{display_name} 跳过接管: {error}");
                Ok(())
            }
        };
        let provider_error = |message: String| TakeoverError::Provider {
            app: app_type.clone(),
            message,
        };

        let Some((id, entry)) = self.select_takeover_entry(app_type, entries, live) else {
            return skip_or(provider_error(format!(
                "{display_name} 无法确定当前使用的供应商条目，无法接管 Live 配置"
            )));
        };
        let provider = match self.db.get_provider_by_id(&id, app_type.as_str()) {
            Ok(Some(provider)) => provider,
            Ok(None) => {
                return skip_or(provider_error(format!(
                    "{display_name} 供应商 {id} 未由 CC Switch 管理，无法接管 Live 配置"
                )))
            }
            Err(e) => {
                return Err(provider_error(format!(
                    "读取 {display_name} 供应商失败: {e}"
                )))
            }
        };
        if !entries.proxyable(&entry) {
            return skip_or(TakeoverError::Projection {
                app: app_type.clone(),
                message: format!(
                    "{display_name} 供应商 {id} 的协议不支持代理接管（仅支持 OpenAI 兼容的 Chat Completions / Responses）"
                ),
            });
        }
        let (upstream, _) =
            crate::apps::adapter(app_type).usage_credentials(&provider.settings_config);
        if upstream.trim().is_empty() {
            return skip_or(provider_error(format!(
                "{display_name} 供应商 {id} 缺少 base_url，无法接管 Live 配置"
            )));
        }

        let endpoint = self
            .bind_entry_token(EntryEndpoint {
                base_url: Some(base_url.to_string()),
                api_key: Some(PROXY_TOKEN_PLACEHOLDER.to_string()),
            })
            .map_err(|message| TakeoverError::Write {
                app: app_type.clone(),
                message,
            })?;
        entries
            .write_endpoint(&id, &endpoint)
            .map_err(|e| TakeoverError::Write {
                app: app_type.clone(),
                message: format!("写入 {display_name} 配置失败: {e}"),
            })?;

        // 累加模式应用没有"当前供应商"界面概念；这里记录被接管的条目，
        // 让代理（故障转移关闭时）与热切换都以它为目标。
        crate::settings::set_current_provider(app_type, Some(id.as_str()))
            .and_then(|()| self.db.set_current_provider(app_type.as_str(), &id))
            .map_err(|e| provider_error(format!("记录 {display_name} 当前供应商失败: {e}")))?;
        log::info!("{display_name} 供应商条目 {id} 已接管");
        Ok(())
    }

    /// 把端点中的 token 哨兵替换为本安装的代理 token
    fn bind_entry_token(&self, mut endpoint: EntryEndpoint) -> Result<EntryEndpoint, String> {
        if endpoint.api_key.as_deref() == Some(LEGACY_PROXY_TOKEN) {
            let token = crate::proxy::inbound_auth::get_or_create_inbound_token(&self.db)
                .map_err(|e| format!("读取代理 token 失败: {e}"))?;
            endpoint.api_key = Some(token);
        }
        Ok(endpoint)
    }

    /// 按 `config`（整份 live 形状：备份或带占位符的 live）写回条目端点。
    ///
    /// 只处理 live 中或 `config` 中带代理 token 的条目，其余条目保持 live 现状：
    /// - 恢复：带占位符的条目写回备份里的地址与 Key；
    /// - 迁移旧版 `PROXY_MANAGED`：条目改写为本安装的 token。
    pub(in crate::services::proxy) fn write_entries_live(
        &self,
        app_type: &AppType,
        config: &Value,
    ) -> Result<(), String> {
        let display_name = registry::descriptor(app_type).display_name;
        let Some(entries) = entry_takeover(app_type) else {
            return Err("该应用不支持代理功能".to_string());
        };
        let live = self.read_entries_live(app_type)?;
        let live_entries = entries.entries(&live);
        for (id, entry) in entries.entries(config) {
            let Some((_, live_entry)) = live_entries.iter().find(|(live_id, _)| *live_id == id)
            else {
                continue;
            };
            let current = entries.endpoint(live_entry);
            let target = entries.endpoint(&entry);
            if !endpoint_has_proxy_token(&current) && !endpoint_has_proxy_token(&target) {
                continue;
            }
            let target = self.bind_entry_token(target)?;
            if target == current {
                continue;
            }
            entries
                .write_endpoint(&id, &target)
                .map_err(|e| format!("写入 {display_name} 配置失败: {e}"))?;
        }
        Ok(())
    }

    /// 无备份兜底：带占位符的条目按 SSOT（同 id 的供应商）写回端点，
    /// 没有可用供应商时删除代理 token 与本地代理地址。
    pub(in crate::services::proxy) fn cleanup_entry_placeholders_in_live(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        let display_name = registry::descriptor(app_type).display_name;
        let Some(entries) = entry_takeover(app_type) else {
            return Ok(());
        };
        let live = self.read_entries_live(app_type)?;
        for (id, entry) in entries.entries(&live) {
            let current = entries.endpoint(&entry);
            if !endpoint_has_proxy_token(&current) {
                continue;
            }
            let from_provider = self
                .db
                .get_provider_by_id(&id, app_type.as_str())
                .ok()
                .flatten()
                .map(|provider| entries.endpoint(&provider.settings_config))
                .filter(|endpoint| !endpoint_has_proxy_token(endpoint));
            let target = from_provider.unwrap_or_else(|| EntryEndpoint {
                base_url: current
                    .base_url
                    .clone()
                    .filter(|url| !Self::is_local_proxy_url(url)),
                api_key: None,
            });
            entries
                .write_endpoint(&id, &target)
                .map_err(|e| format!("清理 {display_name} 接管占位符失败: {e}"))?;
        }
        Ok(())
    }

    /// 累加模式应用的接管快照只在恢复后的 live 与备份完全一致时才逐字节还原：
    /// 接管期间用户可能改了其它条目，整文件还原会把这些改动覆盖掉。
    pub(in crate::services::proxy) async fn entries_live_matches_backup(
        &self,
        app_type: &AppType,
    ) -> bool {
        let backup = match self.db.get_live_backup(app_type.as_str()).await {
            Ok(Some(backup)) => backup,
            _ => return false,
        };
        let Ok(backup) = serde_json::from_str::<Value>(&backup.original_config) else {
            return false;
        };
        self.read_entries_live(app_type)
            .is_ok_and(|live| live == backup)
    }
}

/// 写入累加模式应用的单个供应商条目，接管期间保留代理端点。
///
/// 被接管条目对应的供应商在接管期间被保存（编辑、导入后同步等）时，先按供应商
/// 写入条目，再把此时的 live 记为新的接管备份（恢复时写回用户的新地址与 Key），
/// 最后把条目端点重新指回本地代理。其它条目、未接管的应用直接写入。
pub(crate) fn write_entry_respecting_takeover(
    db: &Database,
    app_type: &AppType,
    provider_id: &str,
    write: impl FnOnce() -> Result<(), AppError>,
) -> Result<(), AppError> {
    let Some(entries) = entry_takeover(app_type) else {
        return write();
    };
    let proxied = crate::apps::adapter(app_type)
        .read_live()
        .ok()
        .and_then(|live| {
            entries
                .entries(&live)
                .into_iter()
                .find(|(id, _)| id == provider_id)
                .map(|(_, entry)| entries.endpoint(&entry))
        })
        .filter(endpoint_has_proxy_token);
    let Some(proxied) = proxied else {
        return write();
    };
    let has_backup = futures::executor::block_on(db.get_live_backup(app_type.as_str()))
        .ok()
        .flatten()
        .is_some();
    if !has_backup {
        return write();
    }

    write()?;
    let live = crate::apps::adapter(app_type).read_live()?;
    let backup = serde_json::to_string(&live)
        .map_err(|e| AppError::Message(format!("序列化 {} 配置失败: {e}", app_type.as_str())))?;
    futures::executor::block_on(db.save_live_backup(app_type.as_str(), &backup))?;
    entries.write_endpoint(provider_id, &proxied)?;
    log::info!(
        "{} 供应商 {provider_id} 在接管期间更新：已刷新接管备份并保留代理地址",
        app_type.as_str()
    );
    Ok(())
}
