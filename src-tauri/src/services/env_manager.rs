use super::env_checker::EnvConflict;
use crate::config::atomic_write;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(not(target_os = "windows"))]
use super::env_checker::{parse_assignment, shell_config_files};
#[cfg(not(target_os = "windows"))]
use std::collections::BTreeMap;

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
    /// Exact rc-file lines removed by the delete, so restore can put them back
    /// verbatim at their original position. Empty in backups written by older
    /// versions; restore then falls back to appending a quoted `export`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_lines: Vec<RemovedLine>,
    /// Conflicts that were *not* removed because cc-switch cannot change their
    /// source (on Unix: variables inherited by the process from launchd,
    /// systemd, `~/.zshenv`, the parent shell, ...). They are excluded from
    /// `conflicts` so restoring the backup never tries to recreate them
    /// (SVC-48).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<SkippedEnv>,
}

/// A conflict `delete_env_vars` left in place, with the reason.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkippedEnv {
    pub var_name: String,
    pub source_type: String,
    pub source_path: String,
    /// Stable machine-readable reason code.
    pub reason: String,
}

/// Reason code: the variable comes from the process environment, which
/// cc-switch cannot edit on Unix.
pub const SKIP_REASON_PROCESS_ENV: &str = "process_environment_unsupported";

/// Split conflicts into those delete can act on and those it cannot.
fn partition_supported(conflicts: Vec<EnvConflict>) -> (Vec<EnvConflict>, Vec<SkippedEnv>) {
    #[cfg(not(target_os = "windows"))]
    {
        let (supported, unsupported): (Vec<_>, Vec<_>) = conflicts
            .into_iter()
            .partition(|c| c.source_type != "system");
        let skipped = unsupported
            .into_iter()
            .map(|c| SkippedEnv {
                var_name: c.var_name,
                source_type: c.source_type,
                source_path: c.source_path,
                reason: SKIP_REASON_PROCESS_ENV.to_string(),
            })
            .collect();
        (supported, skipped)
    }
    #[cfg(target_os = "windows")]
    {
        (conflicts, Vec::new())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemovedLine {
    pub file_path: String,
    /// 1-based line number in the original file.
    pub line_number: usize,
    /// The removed line including its terminator ("\n", "\r\n" or none).
    pub raw_line: String,
}

/// Delete environment variables with automatic backup
pub fn delete_env_vars(conflicts: Vec<EnvConflict>) -> Result<BackupInfo, String> {
    let (conflicts, skipped) = partition_supported(conflicts);
    // Step 1: Work out every rc-file edit up front (and validate it), so a stale
    // or invalid conflict fails before anything is touched.
    #[cfg(not(target_os = "windows"))]
    let edits = plan_file_deletions(&conflicts, &shell_config_files())?;
    #[cfg(not(target_os = "windows"))]
    let removed_lines: Vec<RemovedLine> = edits.iter().flat_map(|e| e.removed.clone()).collect();
    #[cfg(target_os = "windows")]
    let removed_lines: Vec<RemovedLine> = Vec::new();

    // Step 2: Create backup
    let backup_info = create_backup(&conflicts, removed_lines, skipped)?;

    // Step 3: Delete variables
    #[cfg(not(target_os = "windows"))]
    let result = edits
        .iter()
        .try_for_each(|edit| write_rc_file(Path::new(&edit.file_path), &edit.new_content));
    #[cfg(target_os = "windows")]
    let result = conflicts.iter().try_for_each(delete_single_env);

    if let Err(e) = result {
        // If deletion fails, we keep the backup but return error
        return Err(format!(
            "删除环境变量失败: {}. 备份已保存到: {}",
            e, backup_info.backup_path
        ));
    }

    Ok(backup_info)
}

/// Create backup file before deletion
fn create_backup(
    conflicts: &[EnvConflict],
    removed_lines: Vec<RemovedLine>,
    skipped: Vec<SkippedEnv>,
) -> Result<BackupInfo, String> {
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
        removed_lines,
        skipped,
    };

    // Write backup file
    let json = serde_json::to_string_pretty(&backup_info)
        .map_err(|e| format!("序列化备份数据失败: {e}"))?;

    atomic_write(&backup_file, json.as_bytes()).map_err(|e| format!("写入备份文件失败: {e}"))?;

    Ok(backup_info)
}

/// Get backup directory path
fn get_backup_dir() -> Result<PathBuf, String> {
    Ok(crate::config::get_app_config_dir().join("backups"))
}

/// Reject backup files outside cc-switch's own backup directory.
fn ensure_in_dir(path: &Path, dir: &Path) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path).map_err(|e| format!("读取备份文件失败: {e}"))?;
    let dir = fs::canonicalize(dir).map_err(|e| format!("读取备份目录失败: {e}"))?;
    if canonical.starts_with(&dir) {
        Ok(canonical)
    } else {
        Err(format!("备份文件不在备份目录内: {}", path.display()))
    }
}

/// Write an rc file atomically. If the path is a symlink (dotfile managers),
/// the link target is replaced instead of the link itself.
#[cfg(not(target_os = "windows"))]
fn write_rc_file(path: &Path, content: &str) -> Result<(), String> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    atomic_write(&target, content.as_bytes())
        .map_err(|e| format!("写入文件失败 {}: {e}", path.display()))
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

// ========== rc-file editing (Unix) ==========

/// The planned rewrite of one rc file.
#[cfg(not(target_os = "windows"))]
#[derive(Debug)]
struct FileEdit {
    file_path: String,
    new_content: String,
    removed: Vec<RemovedLine>,
}

/// Split `"path:line"` into its parts (the path itself may contain ':').
#[cfg(not(target_os = "windows"))]
fn parse_source_path(source_path: &str) -> Result<(&str, usize), String> {
    let (path, line) = source_path
        .rsplit_once(':')
        .ok_or_else(|| "无效的文件路径格式".to_string())?;
    let line: usize = line
        .parse()
        .map_err(|_| format!("无效的文件路径格式: {source_path}"))?;
    if path.is_empty() || line == 0 {
        return Err(format!("无效的文件路径格式: {source_path}"));
    }
    Ok((path, line))
}

#[cfg(not(target_os = "windows"))]
fn ensure_known_rc_file(path: &str, allowed: &[String]) -> Result<(), String> {
    if allowed.iter().any(|p| p == path) {
        Ok(())
    } else {
        Err(format!("不允许修改的文件: {path}"))
    }
}

#[cfg(not(target_os = "windows"))]
fn strip_eol(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

/// Line terminator to use when one has to be synthesised for `content`.
#[cfg(not(target_os = "windows"))]
fn detect_eol(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Work out which exact lines each file-sourced conflict refers to and the
/// resulting file content. Only the recorded line is removed, and only if it
/// still declares the variable (and still matches the scanned text, when
/// known). Everything else in the file is preserved byte-for-byte.
#[cfg(not(target_os = "windows"))]
fn plan_file_deletions(
    conflicts: &[EnvConflict],
    allowed: &[String],
) -> Result<Vec<FileEdit>, String> {
    let mut by_file: BTreeMap<&str, Vec<(usize, &EnvConflict)>> = BTreeMap::new();
    for conflict in conflicts {
        match conflict.source_type.as_str() {
            "file" => {
                let (path, line) = parse_source_path(&conflict.source_path)?;
                ensure_known_rc_file(path, allowed)?;
                by_file.entry(path).or_default().push((line, conflict));
            }
            // On Unix, we can't directly delete process environment variables
            "system" => {}
            other => return Err(format!("未知的环境变量来源类型: {other}")),
        }
    }

    let mut edits = Vec::new();
    for (path, targets) in by_file {
        let content = fs::read_to_string(path).map_err(|e| format!("读取文件失败 {path}: {e}"))?;
        let lines: Vec<&str> = content.split_inclusive('\n').collect();

        let mut remove = vec![false; lines.len()];
        for (line_number, conflict) in targets {
            let idx = line_number - 1;
            let current = lines.get(idx).map(|l| strip_eol(l));
            let declares = current
                .and_then(parse_assignment)
                .is_some_and(|(name, _)| name == conflict.var_name);
            let unchanged = match (&conflict.original_line, current) {
                (Some(expected), Some(actual)) => expected == actual,
                _ => true,
            };
            if !declares || !unchanged {
                return Err(format!(
                    "{path}:{line_number} 已发生变化，未找到 {}，请重新检测后再试",
                    conflict.var_name
                ));
            }
            remove[idx] = true;
        }

        let mut new_content = String::with_capacity(content.len());
        let mut removed = Vec::new();
        for (idx, line) in lines.iter().enumerate() {
            if remove[idx] {
                removed.push(RemovedLine {
                    file_path: path.to_string(),
                    line_number: idx + 1,
                    raw_line: (*line).to_string(),
                });
            } else {
                new_content.push_str(line);
            }
        }

        edits.push(FileEdit {
            file_path: path.to_string(),
            new_content,
            removed,
        });
    }

    Ok(edits)
}

/// Re-insert removed lines at their original positions. `entries` must belong
/// to the same file; they are applied in ascending line order so each one
/// lands where it was before the (simultaneous) deletion.
#[cfg(not(target_os = "windows"))]
fn reinsert_lines(content: &str, entries: &[&RemovedLine]) -> String {
    let eol = detect_eol(content);
    let mut lines: Vec<String> = content.split_inclusive('\n').map(str::to_string).collect();
    let mut sorted: Vec<&&RemovedLine> = entries.iter().collect();
    sorted.sort_by_key(|e| e.line_number);

    for entry in sorted {
        let idx = entry.line_number.saturating_sub(1).min(lines.len());
        if idx > 0 && !lines[idx - 1].ends_with('\n') {
            lines[idx - 1].push_str(eol);
        }
        let mut raw = entry.raw_line.clone();
        if idx < lines.len() && !raw.ends_with('\n') {
            raw.push_str(eol);
        }
        lines.insert(idx, raw);
    }
    lines.concat()
}

/// POSIX-shell single-quote a value: `a'b` -> `'a'\''b'`.
#[cfg(not(target_os = "windows"))]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(not(target_os = "windows"))]
fn is_valid_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// Append `export NAME='value'` (legacy backups without the original line).
#[cfg(not(target_os = "windows"))]
fn append_export(content: &str, name: &str, value: &str) -> Result<String, String> {
    if !is_valid_var_name(name) {
        return Err(format!("无效的环境变量名: {name}"));
    }
    let eol = detect_eol(content);
    let mut out = content.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(eol);
    }
    out.push_str(&format!("export {name}={}{eol}", shell_single_quote(value)));
    Ok(out)
}

/// Compute the restored content for every rc file touched by a backup.
#[cfg(not(target_os = "windows"))]
fn plan_restore(backup: &BackupInfo, allowed: &[String]) -> Result<Vec<(String, String)>, String> {
    let mut results = Vec::new();

    if !backup.removed_lines.is_empty() {
        let mut by_file: BTreeMap<&str, Vec<&RemovedLine>> = BTreeMap::new();
        for entry in &backup.removed_lines {
            ensure_known_rc_file(&entry.file_path, allowed)?;
            by_file.entry(&entry.file_path).or_default().push(entry);
        }
        for (path, entries) in by_file {
            let content =
                fs::read_to_string(path).map_err(|e| format!("读取文件失败 {path}: {e}"))?;
            results.push((path.to_string(), reinsert_lines(&content, &entries)));
        }
        return Ok(results);
    }

    // Legacy backup: only the (unquoted) value is known. The old delete removed
    // every definition, so append one quoted export per variable and file,
    // using the last definition (the one that took effect).
    let mut by_file: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    for conflict in &backup.conflicts {
        if conflict.source_type != "file" {
            continue;
        }
        let path = conflict
            .source_path
            .rsplit_once(':')
            .map(|(p, _)| p)
            .unwrap_or(&conflict.source_path);
        ensure_known_rc_file(path, allowed)?;
        let vars = by_file.entry(path).or_default();
        vars.retain(|(name, _)| *name != conflict.var_name);
        vars.push((&conflict.var_name, &conflict.var_value));
    }
    for (path, vars) in by_file {
        let mut content =
            fs::read_to_string(path).map_err(|e| format!("读取文件失败 {path}: {e}"))?;
        for (name, value) in vars {
            content = append_export(&content, name, value)?;
        }
        results.push((path.to_string(), content));
    }
    Ok(results)
}

/// Restore environment variables from backup
pub fn restore_from_backup(backup_path: String) -> Result<(), String> {
    let backup_path = ensure_in_dir(Path::new(&backup_path), &get_backup_dir()?)?;

    // Read backup file
    let content = fs::read_to_string(&backup_path).map_err(|e| format!("读取备份文件失败: {e}"))?;

    let backup_info: BackupInfo =
        serde_json::from_str(&content).map_err(|e| format!("解析备份文件失败: {e}"))?;

    #[cfg(not(target_os = "windows"))]
    {
        // Nothing to do for process-environment conflicts on Unix (delete was a no-op).
        for (path, new_content) in plan_restore(&backup_info, &shell_config_files())? {
            write_rc_file(Path::new(&path), &new_content)?;
        }
    }

    #[cfg(target_os = "windows")]
    for conflict in &backup_info.conflicts {
        restore_single_env(conflict)?;
    }

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_dir_creation() {
        let backup_dir = get_backup_dir();
        assert!(backup_dir.is_ok());
    }

    #[test]
    fn backup_path_outside_backup_dir_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let backups = dir.path().join("backups");
        fs::create_dir_all(&backups).unwrap();
        let inside = backups.join("env-backup-1.json");
        let outside = dir.path().join("evil.json");
        fs::write(&inside, "{}").unwrap();
        fs::write(&outside, "{}").unwrap();

        assert!(ensure_in_dir(&inside, &backups).is_ok());
        assert!(ensure_in_dir(&outside, &backups).is_err());
        assert!(ensure_in_dir(&backups.join("../evil.json"), &backups).is_err());
    }

    #[cfg(not(target_os = "windows"))]
    mod rc_files {
        use super::super::*;
        use crate::services::env_checker::scan_shell_config;

        struct Fixture {
            _dir: tempfile::TempDir,
            path: String,
        }

        fn fixture(content: &[u8]) -> Fixture {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(".zshrc");
            fs::write(&path, content).unwrap();
            Fixture {
                path: path.to_string_lossy().to_string(),
                _dir: dir,
            }
        }

        fn scan(f: &Fixture) -> Vec<EnvConflict> {
            let content = fs::read_to_string(&f.path).unwrap();
            scan_shell_config(&f.path, &content, &["ANTHROPIC"])
        }

        /// delete (via plan) then restore (via plan), writing to disk like the
        /// public functions do, but without touching the real home directory.
        fn delete_then_restore(f: &Fixture, conflicts: &[EnvConflict]) -> (String, String) {
            let allowed = vec![f.path.clone()];
            let edits = plan_file_deletions(conflicts, &allowed).unwrap();
            let removed_lines = edits.iter().flat_map(|e| e.removed.clone()).collect();
            for edit in &edits {
                write_rc_file(Path::new(&edit.file_path), &edit.new_content).unwrap();
            }
            let after_delete = fs::read_to_string(&f.path).unwrap();

            let backup = BackupInfo {
                backup_path: String::new(),
                timestamp: String::new(),
                conflicts: conflicts.to_vec(),
                removed_lines,
                skipped: Vec::new(),
            };
            // Round-trip through JSON like a real backup file.
            let backup: BackupInfo =
                serde_json::from_str(&serde_json::to_string(&backup).unwrap()).unwrap();
            for (path, content) in plan_restore(&backup, &allowed).unwrap() {
                write_rc_file(Path::new(&path), &content).unwrap();
            }
            (after_delete, fs::read_to_string(&f.path).unwrap())
        }

        #[test]
        fn delete_first_definition_then_restore_is_byte_identical() {
            let original = "# my zshrc\n\
export ANTHROPIC_API_KEY=\"a b\"\n\
if [ -n \"$WORK\" ]; then\n  export ANTHROPIC_API_KEY='work $HOME; x'\n\
fi\n\
alias ll='ls -l'\n";
            let f = fixture(original.as_bytes());
            let conflicts = scan(&f);
            assert_eq!(conflicts.len(), 2);

            let (after_delete, restored) = delete_then_restore(&f, &conflicts[..1]);
            assert_eq!(
                after_delete,
                "# my zshrc\n\
if [ -n \"$WORK\" ]; then\n  export ANTHROPIC_API_KEY='work $HOME; x'\n\
fi\n\
alias ll='ls -l'\n",
                "only the targeted line is removed; the if-block definition stays"
            );
            assert_eq!(restored.as_bytes(), original.as_bytes());
        }

        #[test]
        fn crlf_and_missing_final_newline_round_trip() {
            let original = "PATH=/bin\r\nexport ANTHROPIC_BASE_URL=http://x\r\nFOO=1\r\nexport ANTHROPIC_API_KEY=k";
            let f = fixture(original.as_bytes());
            let conflicts = scan(&f);
            assert_eq!(conflicts.len(), 2);

            let (after_delete, restored) = delete_then_restore(&f, &conflicts);
            assert_eq!(after_delete, "PATH=/bin\r\nFOO=1\r\n");
            assert_eq!(restored, original);
        }

        #[test]
        fn delete_refuses_when_line_changed_since_scan() {
            let f = fixture(b"export ANTHROPIC_API_KEY=old\n");
            let conflicts = scan(&f);
            fs::write(&f.path, "export ANTHROPIC_API_KEY=new\n").unwrap();
            let err = plan_file_deletions(&conflicts, std::slice::from_ref(&f.path)).unwrap_err();
            assert!(err.contains("ANTHROPIC_API_KEY"), "{err}");

            fs::write(&f.path, "# moved\nexport ANTHROPIC_API_KEY=old\n").unwrap();
            assert!(plan_file_deletions(&conflicts, std::slice::from_ref(&f.path)).is_err());
        }

        #[test]
        fn delete_and_restore_reject_unknown_files() {
            let f = fixture(b"export ANTHROPIC_API_KEY=k\n");
            let conflicts = scan(&f);
            assert!(plan_file_deletions(&conflicts, &["/elsewhere/.zshrc".to_string()]).is_err());

            let backup = BackupInfo {
                backup_path: String::new(),
                timestamp: String::new(),
                conflicts: conflicts.clone(),
                removed_lines: vec![RemovedLine {
                    file_path: f.path.clone(),
                    line_number: 1,
                    raw_line: "export X=1\n".to_string(),
                }],
                skipped: Vec::new(),
            };
            assert!(plan_restore(&backup, &["/elsewhere/.zshrc".to_string()]).is_err());
        }

        #[test]
        fn legacy_backup_restore_appends_single_quoted_value() {
            let f = fixture(b"PATH=/bin");
            let backup: BackupInfo = serde_json::from_value(serde_json::json!({
                "backupPath": "",
                "timestamp": "",
                "conflicts": [
                    {
                        "varName": "ANTHROPIC_API_KEY",
                        "varValue": "first",
                        "sourceType": "file",
                        "sourcePath": format!("{}:2", f.path),
                    },
                    {
                        "varName": "ANTHROPIC_API_KEY",
                        "varValue": "a b $(touch /tmp/pwn); it's",
                        "sourceType": "file",
                        "sourcePath": format!("{}:5", f.path),
                    },
                    {
                        "varName": "ANTHROPIC_MODEL",
                        "varValue": "x",
                        "sourceType": "system",
                        "sourcePath": "Process Environment",
                    }
                ]
            }))
            .unwrap();
            let restored = plan_restore(&backup, std::slice::from_ref(&f.path)).unwrap();
            assert_eq!(restored.len(), 1);
            assert_eq!(
                restored[0].1,
                "PATH=/bin\nexport ANTHROPIC_API_KEY='a b $(touch /tmp/pwn); it'\\''s'\n"
            );
        }

        #[test]
        fn legacy_restore_rejects_invalid_var_name() {
            assert!(append_export("", "FOO;rm -rf ~", "x").is_err());
            assert!(append_export("", "1FOO", "x").is_err());
            assert_eq!(append_export("", "_F1", "").unwrap(), "export _F1=''\n");
        }

        #[test]
        fn source_path_parsing_uses_last_colon() {
            assert_eq!(
                parse_source_path("/home/a:b/.zshrc:12").unwrap(),
                ("/home/a:b/.zshrc", 12)
            );
            assert!(parse_source_path("/home/u/.zshrc").is_err());
            assert!(parse_source_path("/home/u/.zshrc:0").is_err());
        }

        #[test]
        fn process_env_conflicts_are_reported_as_skipped_not_backed_up() {
            let f = fixture(b"export ANTHROPIC_API_KEY=k\n");
            let mut conflicts = scan(&f);
            conflicts.push(EnvConflict {
                var_name: "ANTHROPIC_BASE_URL".to_string(),
                var_value: "http://x".to_string(),
                source_type: "system".to_string(),
                source_path: "Process Environment".to_string(),
                original_line: None,
            });
            let (supported, skipped) = partition_supported(conflicts);
            assert_eq!(supported.len(), 1);
            assert_eq!(supported[0].source_type, "file");
            assert_eq!(skipped.len(), 1);
            assert_eq!(skipped[0].var_name, "ANTHROPIC_BASE_URL");
            assert_eq!(skipped[0].reason, SKIP_REASON_PROCESS_ENV);
        }

        #[test]
        fn backup_mixing_file_and_system_entries_restores_the_file_entry() {
            let original = "export ANTHROPIC_API_KEY=k\nPATH=/bin\n";
            let f = fixture(original.as_bytes());
            let mut conflicts = scan(&f);
            let allowed = vec![f.path.clone()];
            let edits = plan_file_deletions(&conflicts, &allowed).unwrap();
            for edit in &edits {
                write_rc_file(Path::new(&edit.file_path), &edit.new_content).unwrap();
            }
            // An older backup that still lists the process-env entry.
            conflicts.push(EnvConflict {
                var_name: "ANTHROPIC_BASE_URL".to_string(),
                var_value: "http://x".to_string(),
                source_type: "system".to_string(),
                source_path: "Process Environment".to_string(),
                original_line: None,
            });
            let backup = BackupInfo {
                backup_path: String::new(),
                timestamp: String::new(),
                conflicts,
                removed_lines: edits.iter().flat_map(|e| e.removed.clone()).collect(),
                skipped: Vec::new(),
            };
            for (path, content) in plan_restore(&backup, &allowed).unwrap() {
                write_rc_file(Path::new(&path), &content).unwrap();
            }
            assert_eq!(fs::read_to_string(&f.path).unwrap(), original);
        }

        #[test]
        fn symlinked_rc_file_keeps_link() {
            let dir = tempfile::tempdir().unwrap();
            let real = dir.path().join("dotfiles-zshrc");
            fs::write(&real, "export ANTHROPIC_API_KEY=k\nFOO=1\n").unwrap();
            let link = dir.path().join(".zshrc");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            let link_str = link.to_string_lossy().to_string();

            let content = fs::read_to_string(&link).unwrap();
            let conflicts = scan_shell_config(&link_str, &content, &["ANTHROPIC"]);
            let edits = plan_file_deletions(&conflicts, std::slice::from_ref(&link_str)).unwrap();
            write_rc_file(&link, &edits[0].new_content).unwrap();

            assert!(fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink());
            assert_eq!(fs::read_to_string(&real).unwrap(), "FOO=1\n");
        }
    }
}
