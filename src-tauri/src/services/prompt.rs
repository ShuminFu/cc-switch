use indexmap::IndexMap;

use crate::app_config::AppType;
use crate::config::write_text_file;
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::prompt_files::prompt_file_path;
use crate::store::AppState;

/// 安全地获取当前 Unix 时间戳
fn get_unix_timestamp() -> Result<i64, AppError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|e| AppError::Message(format!("Failed to get system time: {e}")))
}

pub struct PromptService;

/// Settings key holding the SHA-256 of the prompt file content cc-switch last
/// wrote for an app (SVC-39). Used to tell an external edit of the live file
/// (which should be backfilled) from content cc-switch put there itself.
fn live_hash_key(app: &AppType) -> String {
    format!("prompt_live_written_hash_{}", app.as_str())
}

fn content_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

impl PromptService {
    /// Write the live prompt file and remember what was written.
    fn write_live(state: &AppState, app: &AppType, content: &str) -> Result<(), AppError> {
        let target_path = prompt_file_path(app)?;
        write_text_file(&target_path, content)?;
        if let Err(e) = state
            .db
            .set_setting(&live_hash_key(app), &content_hash(content))
        {
            log::warn!(
                "Failed to record prompt live hash for {}: {e}",
                app.as_str()
            );
        }
        Ok(())
    }

    /// Whether `live` was changed outside cc-switch since cc-switch last wrote
    /// it. Without a recorded hash (older versions), fall back to comparing
    /// with the enabled prompt's content.
    fn live_edited_externally(
        state: &AppState,
        app: &AppType,
        live: &str,
        enabled: Option<&Prompt>,
    ) -> bool {
        match state.db.get_setting(&live_hash_key(app)).ok().flatten() {
            Some(hash) if !hash.is_empty() => hash != content_hash(live),
            _ => enabled.is_none_or(|p| p.content.trim() != live.trim()),
        }
    }

    pub fn get_prompts(
        state: &AppState,
        app: AppType,
    ) -> Result<IndexMap<String, Prompt>, AppError> {
        state.db.get_prompts(app.as_str())
    }

    pub fn upsert_prompt(
        state: &AppState,
        app: AppType,
        _id: &str,
        prompt: Prompt,
    ) -> Result<(), AppError> {
        // 检查是否为已启用的提示词
        let is_enabled = prompt.enabled;
        // Only a prompt that was enabled before this save "owns" the live
        // file. Saving a prompt that was already disabled (adding the first
        // prompt as disabled, or importing the live file) must never touch the
        // user's existing CLAUDE.md / AGENTS.md.
        let was_enabled = state
            .db
            .get_prompts(app.as_str())?
            .get(&prompt.id)
            .is_some_and(|p| p.enabled);

        if is_enabled {
            // 启用提示词：先写文件再保存 DB，文件写入失败时 DB 不变 (SVC-39)
            Self::write_live(state, &app, &prompt.content)?;
            state.db.save_prompt(app.as_str(), &prompt)?;
            return Ok(());
        }

        state.db.save_prompt(app.as_str(), &prompt)?;

        if was_enabled {
            // 禁用提示词：检查是否还有其他已启用的提示词
            let prompts = state.db.get_prompts(app.as_str())?;
            let any_enabled = prompts.values().any(|p| p.enabled);

            if !any_enabled {
                // 所有提示词都已禁用，清空文件
                let target_path = prompt_file_path(&app)?;
                if target_path.exists() {
                    Self::write_live(state, &app, "")?;
                }
            }
        }

        Ok(())
    }

    pub fn delete_prompt(state: &AppState, app: AppType, id: &str) -> Result<(), AppError> {
        let prompts = state.db.get_prompts(app.as_str())?;

        if let Some(prompt) = prompts.get(id) {
            if prompt.enabled {
                return Err(AppError::InvalidInput("无法删除已启用的提示词".to_string()));
            }
        }

        state.db.delete_prompt(app.as_str(), id)?;
        Ok(())
    }

    pub fn enable_prompt(state: &AppState, app: AppType, id: &str) -> Result<(), AppError> {
        // 回填当前 live 文件内容到已启用的提示词，或创建备份
        let target_path = prompt_file_path(&app)?;
        if target_path.exists() {
            if let Ok(live_content) = std::fs::read_to_string(&target_path) {
                let mut prompts = state.db.get_prompts(app.as_str())?;
                let externally_edited = Self::live_edited_externally(
                    state,
                    &app,
                    &live_content,
                    prompts.values().find(|p| p.enabled),
                );
                // Only backfill an external edit: content cc-switch wrote
                // itself must not overwrite a newer UI edit, nor spawn backup
                // prompts on every toggle (SVC-39).
                if externally_edited && !live_content.trim().is_empty() {
                    // 尝试回填到当前已启用的提示词
                    if let Some((enabled_id, enabled_prompt)) = prompts
                        .iter_mut()
                        .find(|(_, p)| p.enabled)
                        .map(|(id, p)| (id.clone(), p))
                    {
                        let timestamp = get_unix_timestamp()?;
                        enabled_prompt.content = live_content.clone();
                        enabled_prompt.updated_at = Some(timestamp);
                        log::info!("回填 live 提示词内容到已启用项: {enabled_id}");
                        state.db.save_prompt(app.as_str(), enabled_prompt)?;
                    } else {
                        // 没有已启用的提示词，则创建一次备份（避免重复备份）
                        let content_exists = prompts
                            .values()
                            .any(|p| p.content.trim() == live_content.trim());
                        if !content_exists {
                            let timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs() as i64;
                            let backup_id = format!("backup-{timestamp}");
                            let backup_prompt = Prompt {
                                id: backup_id.clone(),
                                name: format!(
                                    "原始提示词 {}",
                                    chrono::Local::now().format("%Y-%m-%d %H:%M")
                                ),
                                content: live_content,
                                description: Some("自动备份的原始提示词".to_string()),
                                enabled: false,
                                created_at: Some(timestamp),
                                updated_at: Some(timestamp),
                            };
                            log::info!("回填 live 提示词内容，创建备份: {backup_id}");
                            state.db.save_prompt(app.as_str(), &backup_prompt)?;
                        }
                    }
                }
            }
        }

        // 启用目标提示词并写入文件
        let prompts = state.db.get_prompts(app.as_str())?;
        let Some(target) = prompts.get(id).cloned() else {
            return Err(AppError::InvalidInput(format!("提示词 {id} 不存在")));
        };
        let previous_live = std::fs::read_to_string(&target_path).ok();

        Self::write_live(state, &app, &target.content)?; // 原子写入

        // Disable the others first, then enable the target: a failure part
        // way leaves at most one enabled prompt, never two. Only rows whose
        // flag changes are saved.
        let result = (|| {
            for prompt in prompts.values().filter(|p| p.enabled && p.id != id) {
                let mut disabled = prompt.clone();
                disabled.enabled = false;
                state.db.save_prompt(app.as_str(), &disabled)?;
            }
            if !target.enabled {
                let mut enabled = target.clone();
                enabled.enabled = true;
                state.db.save_prompt(app.as_str(), &enabled)?;
            }
            Ok::<(), AppError>(())
        })();
        if let Err(err) = result {
            if let Some(previous) = previous_live {
                if let Err(e) = Self::write_live(state, &app, &previous) {
                    log::warn!("Failed to restore prompt file after DB error: {e}");
                }
            }
            return Err(err);
        }

        Ok(())
    }

    pub fn import_from_file(state: &AppState, app: AppType) -> Result<String, AppError> {
        let file_path = prompt_file_path(&app)?;

        if !file_path.exists() {
            return Err(AppError::Message("提示词文件不存在".to_string()));
        }

        let content =
            std::fs::read_to_string(&file_path).map_err(|e| AppError::io(&file_path, e))?;
        let timestamp = get_unix_timestamp()?;

        let id = format!("imported-{timestamp}");
        let prompt = Prompt {
            id: id.clone(),
            name: format!(
                "导入的提示词 {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M")
            ),
            content,
            description: Some("从现有配置文件导入".to_string()),
            enabled: false,
            created_at: Some(timestamp),
            updated_at: Some(timestamp),
        };

        Self::upsert_prompt(state, app, &id, prompt)?;
        Ok(id)
    }

    pub fn get_current_file_content(app: AppType) -> Result<Option<String>, AppError> {
        let file_path = prompt_file_path(&app)?;
        if !file_path.exists() {
            return Ok(None);
        }
        let content =
            std::fs::read_to_string(&file_path).map_err(|e| AppError::io(&file_path, e))?;
        Ok(Some(content))
    }

    /// 首次启动时从现有提示词文件自动导入（如果存在）
    /// 返回导入的数量
    pub fn import_from_file_on_first_launch(
        state: &AppState,
        app: AppType,
    ) -> Result<usize, AppError> {
        // 幂等性保护：该应用已有提示词则跳过
        let existing = state.db.get_prompts(app.as_str())?;
        if !existing.is_empty() {
            return Ok(0);
        }

        let file_path = prompt_file_path(&app)?;

        // 检查文件是否存在
        if !file_path.exists() {
            return Ok(0);
        }

        // 读取文件内容
        let content = match std::fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("读取提示词文件失败: {file_path:?}, 错误: {e}");
                return Ok(0);
            }
        };

        // 检查内容是否为空
        if content.trim().is_empty() {
            return Ok(0);
        }

        log::info!("发现提示词文件，自动导入: {file_path:?}");

        // 创建提示词对象
        let timestamp = get_unix_timestamp()?;
        let id = format!("auto-imported-{timestamp}");
        let prompt = Prompt {
            id: id.clone(),
            name: format!(
                "Auto-imported Prompt {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M")
            ),
            content,
            description: Some("Automatically imported on first launch".to_string()),
            enabled: true, // 首次导入时自动启用
            created_at: Some(timestamp),
            updated_at: Some(timestamp),
        };

        // 保存到数据库
        state.db.save_prompt(app.as_str(), &prompt)?;

        log::info!("自动导入完成: {}", app.as_str());
        Ok(1)
    }
}
