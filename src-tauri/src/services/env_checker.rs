use serde::{Deserialize, Serialize};
#[cfg(not(target_os = "windows"))]
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvConflict {
    pub var_name: String,
    pub var_value: String,
    pub source_type: String, // "system" | "file"
    pub source_path: String, // Registry path or file path
    /// Exact text of the rc-file line this conflict was read from (without the
    /// line terminator). Used to verify the line is unchanged before deleting it.
    /// Absent for system/registry conflicts and for older clients/backups.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_line: Option<String>,
}

#[cfg(target_os = "windows")]
use winreg::enums::*;
#[cfg(target_os = "windows")]
use winreg::RegKey;

/// Check environment variables for conflicts
pub fn check_env_conflicts(app: &str) -> Result<Vec<EnvConflict>, String> {
    let keywords = get_keywords_for_app(app);
    let mut conflicts = Vec::new();

    // Check system environment variables
    conflicts.extend(check_system_env(&keywords)?);

    // Check shell configuration files (Unix only)
    #[cfg(not(target_os = "windows"))]
    conflicts.extend(check_shell_configs(&keywords)?);

    Ok(conflicts)
}

/// Get relevant keywords for each app
fn get_keywords_for_app(app: &str) -> Vec<&str> {
    match app.to_lowercase().as_str() {
        "claude" => vec!["ANTHROPIC"],
        "codex" => vec!["OPENAI"],
        "gemini" => vec!["GEMINI", "GOOGLE_GEMINI"],
        _ => vec![],
    }
}

/// Check system environment variables (Windows Registry or Unix env)
#[cfg(target_os = "windows")]
fn check_system_env(keywords: &[&str]) -> Result<Vec<EnvConflict>, String> {
    let mut conflicts = Vec::new();

    // Check HKEY_CURRENT_USER\Environment
    if let Ok(hkcu) = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Environment") {
        for (name, value) in hkcu.enum_values().filter_map(Result::ok) {
            if keywords.iter().any(|k| name.to_uppercase().contains(k)) {
                conflicts.push(EnvConflict {
                    var_name: name.clone(),
                    var_value: value.to_string(),
                    source_type: "system".to_string(),
                    source_path: "HKEY_CURRENT_USER\\Environment".to_string(),
                    original_line: None,
                });
            }
        }
    }

    // Check HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Session Manager\Environment
    if let Ok(hklm) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment")
    {
        for (name, value) in hklm.enum_values().filter_map(Result::ok) {
            if keywords.iter().any(|k| name.to_uppercase().contains(k)) {
                conflicts.push(EnvConflict {
                    var_name: name.clone(),
                    var_value: value.to_string(),
                    source_type: "system".to_string(),
                    source_path: "HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment".to_string(),
                    original_line: None,
                });
            }
        }
    }

    Ok(conflicts)
}

#[cfg(not(target_os = "windows"))]
fn check_system_env(keywords: &[&str]) -> Result<Vec<EnvConflict>, String> {
    let mut conflicts = Vec::new();

    // Check current process environment
    for (key, value) in std::env::vars() {
        if keywords.iter().any(|k| key.to_uppercase().contains(k)) {
            conflicts.push(EnvConflict {
                var_name: key,
                var_value: value,
                source_type: "system".to_string(),
                source_path: "Process Environment".to_string(),
                original_line: None,
            });
        }
    }

    Ok(conflicts)
}

/// The shell rc files cc-switch scans for conflicting exports (Unix only).
///
/// This is also the allow-list of files that delete/restore may rewrite.
#[cfg(not(target_os = "windows"))]
pub(crate) fn shell_config_files() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    vec![
        format!("{}/.bashrc", home),
        format!("{}/.bash_profile", home),
        format!("{}/.zshrc", home),
        format!("{}/.zprofile", home),
        format!("{}/.profile", home),
        "/etc/profile".to_string(),
        "/etc/bashrc".to_string(),
    ]
}

/// If `line` assigns a variable (`export NAME=value` or `NAME=value`), return
/// `(name, raw_value)`. Comment lines are ignored.
#[cfg(not(target_os = "windows"))]
pub(crate) fn parse_assignment(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if trimmed.starts_with('#') {
        return None;
    }
    let export_line = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    let eq_pos = export_line.find('=')?;
    Some((
        export_line[..eq_pos].trim(),
        export_line[eq_pos + 1..].trim(),
    ))
}

/// Scan one rc file's content for assignments whose name contains a keyword.
#[cfg(not(target_os = "windows"))]
pub(crate) fn scan_shell_config(
    file_path: &str,
    content: &str,
    keywords: &[&str],
) -> Vec<EnvConflict> {
    let mut conflicts = Vec::new();
    // `str::lines` strips both "\n" and "\r\n", so line numbers match the
    // `split_inclusive('\n')` indexing used by env_manager.
    for (line_num, line) in content.lines().enumerate() {
        let Some((var_name, var_value)) = parse_assignment(line) else {
            continue;
        };
        if keywords.iter().any(|k| var_name.to_uppercase().contains(k)) {
            conflicts.push(EnvConflict {
                var_name: var_name.to_string(),
                var_value: var_value.trim_matches('"').trim_matches('\'').to_string(),
                source_type: "file".to_string(),
                source_path: format!("{}:{}", file_path, line_num + 1),
                original_line: Some(line.to_string()),
            });
        }
    }
    conflicts
}

/// Check shell configuration files for environment variable exports (Unix only)
#[cfg(not(target_os = "windows"))]
fn check_shell_configs(keywords: &[&str]) -> Result<Vec<EnvConflict>, String> {
    let mut conflicts = Vec::new();

    for file_path in shell_config_files() {
        if let Ok(content) = fs::read_to_string(&file_path) {
            conflicts.extend(scan_shell_config(&file_path, &content, keywords));
        }
    }

    Ok(conflicts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_keywords() {
        assert_eq!(get_keywords_for_app("claude"), vec!["ANTHROPIC"]);
        assert_eq!(get_keywords_for_app("codex"), vec!["OPENAI"]);
        assert_eq!(
            get_keywords_for_app("gemini"),
            vec!["GEMINI", "GOOGLE_GEMINI"]
        );
        assert_eq!(get_keywords_for_app("unknown"), Vec::<&str>::new());
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn scan_records_line_number_and_original_text() {
        let content =
            "# ANTHROPIC_API_KEY=commented\r\nexport ANTHROPIC_API_KEY=\"a b\"\r\nPATH=/bin\r\n";
        let conflicts = scan_shell_config("/home/u/.zshrc", content, &["ANTHROPIC"]);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].var_name, "ANTHROPIC_API_KEY");
        assert_eq!(conflicts[0].var_value, "a b");
        assert_eq!(conflicts[0].source_path, "/home/u/.zshrc:2");
        assert_eq!(
            conflicts[0].original_line.as_deref(),
            Some("export ANTHROPIC_API_KEY=\"a b\"")
        );
    }
}
