//! Claude Code subagents (`~/.claude/agents/*.md`), PRODUCT-42 first increment:
//! list the user-level agents and create new ones. Each agent is a Markdown
//! file with YAML frontmatter (`name`, `description`, optional `model` and
//! `tools`) followed by the system prompt. Existing files are never
//! overwritten.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::run_blocking;
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAgentInfo {
    /// File name, e.g. `code-reviewer.md`.
    pub file_name: String,
    pub path: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<String>,
    /// The Markdown body (the agent's system prompt).
    pub prompt: String,
    /// Frontmatter missing or unparsable; Claude Code may ignore the file.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invalid: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewClaudeAgent {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub tools: Option<String>,
    #[serde(default)]
    pub prompt: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Frontmatter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tools: Option<serde_yaml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
}

fn agents_dir() -> PathBuf {
    crate::config::get_claude_config_dir().join("agents")
}

/// Split `---\n<yaml>\n---\n<body>`; `None` when there is no frontmatter.
fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            let body = &rest[offset + line.len()..];
            return Some((&rest[..offset], body));
        }
        offset += line.len();
    }
    None
}

fn tools_to_string(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Sequence(items) => Some(
            items
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ),
        _ => None,
    }
}

fn parse_agent(path: &Path, text: &str) -> ClaudeAgentInfo {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = file_name.trim_end_matches(".md").to_string();
    let parsed = split_frontmatter(text)
        .and_then(|(yaml, body)| Some((serde_yaml::from_str::<Frontmatter>(yaml).ok()?, body)));
    match parsed {
        Some((fm, body)) => ClaudeAgentInfo {
            invalid: fm.name.is_none() || fm.description.is_none(),
            name: fm.name.unwrap_or(stem),
            description: fm.description,
            model: fm.model,
            tools: fm.tools.as_ref().and_then(tools_to_string),
            prompt: body.trim_start_matches(['\r', '\n']).to_string(),
            path: path.display().to_string(),
            file_name,
        },
        None => ClaudeAgentInfo {
            name: stem,
            description: None,
            model: None,
            tools: None,
            prompt: text.to_string(),
            invalid: true,
            path: path.display().to_string(),
            file_name,
        },
    }
}

fn list_in(dir: &Path) -> Result<Vec<ClaudeAgentInfo>, AppError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(AppError::io(dir, e)),
    };
    let mut agents = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
        agents.push(parse_agent(&path, &text));
    }
    agents.sort_by_key(|a| a.name.to_lowercase());
    Ok(agents)
}

/// Agent names Claude Code accepts: lowercase letters, digits and hyphens.
fn is_valid_agent_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
}

fn render_agent(agent: &NewClaudeAgent) -> Result<String, AppError> {
    let fm = Frontmatter {
        name: Some(agent.name.clone()),
        description: Some(agent.description.trim().to_string()),
        tools: agent
            .tools
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| serde_yaml::Value::String(t.to_string())),
        model: agent
            .model
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_string),
    };
    let yaml = serde_yaml::to_string(&fm)
        .map_err(|e| AppError::Message(format!("YAML serialization failed: {e}")))?;
    let body = agent.prompt.trim();
    Ok(format!("---\n{yaml}---\n\n{body}\n"))
}

fn create_in(dir: &Path, agent: &NewClaudeAgent) -> Result<ClaudeAgentInfo, AppError> {
    if !is_valid_agent_name(&agent.name) {
        return Err(AppError::localized(
            "agents.invalid_name",
            format!("代理名称无效: {}（仅限小写字母、数字和连字符）", agent.name),
            format!(
                "Invalid agent name \"{}\": use lowercase letters, digits and hyphens.",
                agent.name
            ),
        ));
    }
    if agent.description.trim().is_empty() {
        return Err(AppError::localized(
            "agents.description_required",
            "代理描述不能为空",
            "An agent needs a description.",
        ));
    }
    let path = dir.join(format!("{}.md", agent.name));
    if path.exists() {
        return Err(AppError::localized(
            "agents.already_exists",
            format!("代理 {} 已存在", agent.name),
            format!("An agent named {} already exists.", agent.name),
        ));
    }
    let text = render_agent(agent)?;
    crate::config::write_text_file(&path, &text)?;
    Ok(parse_agent(&path, &text))
}

/// User-level Claude Code subagents in `~/.claude/agents`.
#[tauri::command]
pub async fn list_claude_agents() -> Result<Vec<ClaudeAgentInfo>, String> {
    run_blocking(move || list_in(&agents_dir()))
        .await
        .map_err(|e: AppError| e.to_string())
}

/// Create `~/.claude/agents/<name>.md`; refuses to overwrite an existing file.
#[tauri::command]
pub async fn create_claude_agent(agent: NewClaudeAgent) -> Result<ClaudeAgentInfo, String> {
    run_blocking(move || create_in(&agents_dir(), &agent))
        .await
        .map_err(|e: AppError| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(name: &str) -> NewClaudeAgent {
        NewClaudeAgent {
            name: name.to_string(),
            description: "Reviews diffs: finds bugs".to_string(),
            model: Some("sonnet".to_string()),
            tools: Some("Read, Grep".to_string()),
            prompt: "You review code.\n".to_string(),
        }
    }

    #[test]
    fn created_agent_has_valid_frontmatter_and_lists_back() {
        let dir = tempfile::tempdir().unwrap();
        let created = create_in(dir.path(), &agent("code-reviewer")).unwrap();
        assert!(!created.invalid);
        let text = std::fs::read_to_string(dir.path().join("code-reviewer.md")).unwrap();
        let (yaml, body) = split_frontmatter(&text).unwrap();
        let fm: Frontmatter = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(fm.name.as_deref(), Some("code-reviewer"));
        // A colon in the description is quoted, not parsed as a mapping.
        assert_eq!(fm.description.as_deref(), Some("Reviews diffs: finds bugs"));
        assert_eq!(body.trim(), "You review code.");

        let listed = list_in(dir.path()).unwrap();
        assert_eq!(listed, vec![created]);
        assert_eq!(listed[0].tools.as_deref(), Some("Read, Grep"));
        assert_eq!(listed[0].model.as_deref(), Some("sonnet"));
    }

    #[test]
    fn create_refuses_bad_names_and_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(create_in(dir.path(), &agent("Bad Name")).is_err());
        assert!(create_in(dir.path(), &agent("../escape")).is_err());
        let mut no_desc = agent("ok");
        no_desc.description = " ".to_string();
        assert!(create_in(dir.path(), &no_desc).is_err());

        std::fs::write(dir.path().join("taken.md"), "user content").unwrap();
        assert!(create_in(dir.path(), &agent("taken")).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("taken.md")).unwrap(),
            "user content"
        );
    }

    #[test]
    fn lists_files_without_frontmatter_as_invalid_and_missing_dir_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list_in(&dir.path().join("missing")).unwrap().is_empty());
        std::fs::write(dir.path().join("plain.md"), "just text").unwrap();
        std::fs::write(
            dir.path().join("multi.md"),
            "---\nname: multi\ndescription: d\ntools:\n  - Read\n  - Bash\n---\nbody\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("notes.txt"), "ignored").unwrap();
        let listed = list_in(dir.path()).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "multi");
        assert_eq!(listed[0].tools.as_deref(), Some("Read, Bash"));
        assert!(listed[1].invalid);
        assert_eq!(listed[1].name, "plain");
    }
}
