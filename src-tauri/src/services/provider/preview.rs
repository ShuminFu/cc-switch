//! Live 写入预览
//!
//! 保存供应商前，按 [`ProviderService::update`] / [`ProviderService::add`] 的同一套
//! 判定与写入流程跑一遍，但写入经 [`crate::config::capture_writes`] 只被捕获、不落盘，
//! 返回每个目标文件当前内容与将写入的内容，供前端渲染 diff。

use std::path::PathBuf;

use serde::Serialize;

use crate::app_config::AppType;
use crate::config::{capture_writes, CapturedWrite};
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::mcp::McpService;
use crate::store::AppState;

use super::live::{
    normalize_provider_common_config_for_storage, takeover_uses_live_backup,
    write_live_with_common_config,
};
use super::ProviderService;

/// 保存时不写 live 的原因
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(specta::Type))]
pub enum LivePreviewSkipReason {
    /// 切换模式应用：该供应商不是当前供应商
    NotCurrent,
    /// 累加模式应用：该供应商未加入应用配置
    NotInLive,
    /// 代理接管期间保存只更新接管备份
    ProxyTakeover,
}

/// 单个 live 文件的写入前后内容；`None` 表示文件不存在 / 将被删除
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(specta::Type))]
pub struct LiveFilePreview {
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// 保存供应商将对 live 配置做的改动
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(specta::Type))]
pub struct LiveWritePreview {
    /// 保存时是否会写入 live 配置
    pub writes_live: bool,
    /// 不写 live 时的原因
    pub skip_reason: Option<LivePreviewSkipReason>,
    /// 按首次写入顺序排列的目标文件（每个路径一项，内容为最终写入结果）
    pub files: Vec<LiveFilePreview>,
}

impl LiveWritePreview {
    fn skipped(reason: LivePreviewSkipReason) -> Self {
        Self {
            writes_live: false,
            skip_reason: Some(reason),
            files: Vec::new(),
        }
    }
}

impl ProviderService {
    /// 预览保存 `provider` 时写入 live 配置的内容，不修改任何 live 文件。
    ///
    /// `original_id` 与 [`ProviderService::update`] 相同；数据库中不存在该供应商时
    /// 按新增（[`ProviderService::add`]，加入应用配置）处理。
    pub fn preview_live_write(
        state: &AppState,
        app_type: AppType,
        original_id: Option<&str>,
        provider: Provider,
    ) -> Result<LiveWritePreview, AppError> {
        let mut provider = provider;
        let original_id = original_id.unwrap_or(provider.id.as_str()).to_string();
        let existing_provider = state
            .db
            .get_provider_by_id(&original_id, app_type.as_str())?;

        // 与保存一致的规范化与校验，预览的就是将落库并写入 live 的那份配置
        Self::normalize_provider_settings(&app_type, &mut provider);
        Self::validate_provider_settings(&app_type, &provider)?;
        normalize_provider_common_config_for_storage(state.db.as_ref(), &app_type, &mut provider)?;
        Self::normalize_usage_script_credential_overrides(&app_type, &mut provider);

        if app_type.is_additive_mode() {
            return Self::preview_additive(
                state,
                &app_type,
                existing_provider,
                original_id,
                provider,
            );
        }

        if existing_provider.is_none() {
            // 新增：仅在没有当前供应商时设为当前并写入 live
            if state.db.get_current_provider(app_type.as_str())?.is_some() {
                return Ok(LiveWritePreview::skipped(LivePreviewSkipReason::NotCurrent));
            }
            return Self::capture_live_write(state, &app_type, &provider, false);
        }

        let effective_current =
            crate::settings::get_effective_current_provider(&state.db, &app_type)?;
        if effective_current.as_deref() != Some(provider.id.as_str()) {
            return Ok(LiveWritePreview::skipped(LivePreviewSkipReason::NotCurrent));
        }

        let has_live_backup =
            futures::executor::block_on(state.db.get_live_backup(app_type.as_str()))
                .ok()
                .flatten()
                .is_some();
        let live_taken_over = state
            .proxy_service
            .detect_takeover_in_live_config_for_app(&app_type);
        if has_live_backup || live_taken_over {
            if takeover_uses_live_backup(&app_type) {
                return Ok(LiveWritePreview::skipped(
                    LivePreviewSkipReason::ProxyTakeover,
                ));
            }
            return Self::capture_live_write(state, &app_type, &provider, false);
        }

        // 普通保存在写 live 后重投影本应用的 MCP（Codex / Grok 的 config.toml、
        // Gemini 的 settings.json 与供应商配置同文件），预览需要同样走一遍
        Self::capture_live_write(state, &app_type, &provider, true)
    }

    fn preview_additive(
        state: &AppState,
        app_type: &AppType,
        existing_provider: Option<Provider>,
        original_id: String,
        provider: Provider,
    ) -> Result<LiveWritePreview, AppError> {
        // 改名只更新数据库（已在 live 中的供应商不允许改名）
        if original_id != provider.id {
            return Ok(LiveWritePreview::skipped(LivePreviewSkipReason::NotInLive));
        }

        if let Some(variant) = Self::omo_variant(app_type, &provider) {
            // 新增的 OMO 供应商不会自动启用；已有的只在当前启用时写专用配置文件
            let is_current = existing_provider.is_some()
                && state.db.is_omo_provider_current(
                    app_type.as_str(),
                    &provider.id,
                    variant.category,
                )?;
            if !is_current {
                return Ok(LiveWritePreview::skipped(LivePreviewSkipReason::NotCurrent));
            }
            let (result, writes) = capture_writes(|| {
                crate::services::OmoService::write_provider_config_to_file(&provider, variant)
            });
            result?;
            return Ok(build_preview(writes));
        }

        if let Some(existing) = existing_provider.as_ref() {
            let live_config_managed = Self::check_live_config_exists(
                app_type,
                &provider.id,
                Self::provider_live_config_managed(&provider)
                    .or_else(|| Self::provider_live_config_managed(existing)),
            )?;
            if !live_config_managed {
                return Ok(LiveWritePreview::skipped(LivePreviewSkipReason::NotInLive));
            }
        }

        Self::capture_live_write(state, app_type, &provider, false)
    }

    /// 在捕获模式下执行写 live（可选随后重投影 MCP），汇总为预览。
    ///
    /// 漂移检测（[`crate::services::drift`]）用它取得当前供应商的预期 live 输出。
    pub(crate) fn capture_live_write(
        state: &AppState,
        app_type: &AppType,
        provider: &Provider,
        resync_mcp: bool,
    ) -> Result<LiveWritePreview, AppError> {
        // 内层捕获只记录供应商写入；其记录同时并入外层，后续 MCP 重投影
        // 读文件时能看到这些待写内容
        let (result, writes) = capture_writes(|| -> Result<Vec<PathBuf>, AppError> {
            let (result, live_writes) = capture_writes(|| {
                write_live_with_common_config(state.db.as_ref(), app_type, provider)
            });
            result?;
            if resync_mcp {
                // 与保存一致，投影失败只记录警告
                if let Err(err) = McpService::sync_enabled_for_app(state, app_type) {
                    log::warn!("预览 {app_type:?} 写入时重投影 MCP 失败: {err}");
                }
            }
            Ok(live_writes.into_iter().map(|write| write.path).collect())
        });
        let touched = result?;

        // MCP 重投影只在供应商写入的同一批文件上计入预览；写到别的文件（如 Claude
        // 的 ~/.claude.json）的投影与本次保存的内容无关
        Ok(build_preview(
            writes
                .into_iter()
                .filter(|write| touched.contains(&write.path))
                .collect(),
        ))
    }
}

/// 汇总捕获到的写入：每个路径保留最后一次写入的内容，按首次出现排序
fn build_preview(writes: Vec<CapturedWrite>) -> LiveWritePreview {
    let mut files: Vec<(PathBuf, Option<Vec<u8>>)> = Vec::new();
    for write in writes {
        match files.iter_mut().find(|(path, _)| *path == write.path) {
            Some(entry) => entry.1 = write.contents,
            None => files.push((write.path, write.contents)),
        }
    }

    LiveWritePreview {
        writes_live: true,
        skip_reason: None,
        files: files
            .into_iter()
            .map(|(path, after)| LiveFilePreview {
                before: std::fs::read(&path)
                    .ok()
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
                after: after.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
                path: path.display().to_string(),
            })
            .collect(),
    }
}
