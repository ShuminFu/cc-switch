//! Render a provider as a shell environment profile (SVC-35).
//!
//! Lets users reuse a cc-switch provider where the CLI's live files are not
//! available (CI, containers, a shell on another machine): the output is a
//! block of properly quoted `export` lines (or the fish / PowerShell /
//! `.env` equivalent) that sets the variables the CLI reads.

use serde::{Deserialize, Serialize};

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;

/// Output syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvShell {
    /// POSIX sh / bash / zsh: `export NAME='value'`.
    Bash,
    /// fish: `set -gx NAME 'value'`.
    Fish,
    /// PowerShell: `$env:NAME = 'value'`.
    PowerShell,
    /// `.env` / direnv `dotenv` file: `NAME='value'`.
    Dotenv,
}

fn is_valid_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn env_object_vars(provider: &Provider) -> Vec<(String, String)> {
    provider
        .settings_config
        .get("env")
        .and_then(|env| env.as_object())
        .map(|env| {
            env.iter()
                .filter_map(|(k, v)| {
                    let value = match v {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        _ => return None,
                    };
                    Some((k.clone(), value))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The environment variables that configure `app` for `provider`.
pub fn env_vars_for(app: &AppType, provider: &Provider) -> Result<Vec<(String, String)>, AppError> {
    let vars = match app {
        // Claude Code and Gemini CLI read their whole `env` block from the
        // environment as well.
        AppType::Claude | AppType::Gemini => env_object_vars(provider),
        AppType::Codex => {
            let (base_url, api_key) = provider.resolve_usage_credentials(app);
            let mut vars = Vec::new();
            if !api_key.is_empty() {
                vars.push(("OPENAI_API_KEY".to_string(), api_key));
            }
            if !base_url.is_empty() {
                vars.push(("OPENAI_BASE_URL".to_string(), base_url));
            }
            vars
        }
        AppType::GrokBuild => {
            let (_, api_key) = provider.resolve_usage_credentials(app);
            if api_key.is_empty() {
                Vec::new()
            } else {
                vec![("XAI_API_KEY".to_string(), api_key)]
            }
        }
        _ => {
            return Err(AppError::localized(
                "env_profile.unsupported_app",
                format!("{} 不支持导出为环境变量", app.as_str()),
                format!(
                    "{} providers cannot be exported as environment variables.",
                    app.as_str()
                ),
            ))
        }
    };
    let vars: Vec<_> = vars
        .into_iter()
        .filter(|(name, _)| is_valid_var_name(name))
        .collect();
    if vars.is_empty() {
        return Err(AppError::localized(
            "env_profile.no_variables",
            "该供应商没有可导出的环境变量",
            "This provider has no environment variables to export.",
        ));
    }
    Ok(vars)
}

/// POSIX single quoting: `a'b` → `'a'\''b'`.
fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// fish single quoting: only `\` and `'` are special inside quotes.
fn fish_quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// PowerShell verbatim string: `'` is doubled.
fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// `.env` value: single-quoted when possible (no expansion), otherwise
/// double-quoted with `\`, `"`, `$` and newlines escaped.
fn dotenv_quote(value: &str) -> String {
    if !value.contains('\'') && !value.contains('\n') {
        return format!("'{value}'");
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Render `vars` in `shell` syntax, one assignment per line.
pub fn render_vars(vars: &[(String, String)], shell: EnvShell) -> String {
    let mut out = String::new();
    for (name, value) in vars {
        let line = match shell {
            EnvShell::Bash => format!("export {name}={}", sh_quote(value)),
            EnvShell::Fish => format!("set -gx {name} {}", fish_quote(value)),
            EnvShell::PowerShell => format!("$env:{name} = {}", powershell_quote(value)),
            EnvShell::Dotenv => format!("{name}={}", dotenv_quote(value)),
        };
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Render `provider` for `app` as a shell profile.
pub fn render(app: &AppType, provider: &Provider, shell: EnvShell) -> Result<String, AppError> {
    Ok(render_vars(&env_vars_for(app, provider)?, shell))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn claude_provider(token: &str) -> Provider {
        Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.example.com",
                    "ANTHROPIC_AUTH_TOKEN": token,
                    "bad name": "ignored"
                }
            }),
            None,
        )
    }

    #[test]
    fn claude_profile_sets_base_url_and_token() {
        let out = render(&AppType::Claude, &claude_provider("sk-1"), EnvShell::Bash).unwrap();
        assert!(out.contains("export ANTHROPIC_BASE_URL='https://api.example.com'\n"));
        assert!(out.contains("export ANTHROPIC_AUTH_TOKEN='sk-1'\n"));
        assert!(!out.contains("bad name"));
    }

    #[cfg(unix)]
    #[test]
    fn bash_quoting_round_trips_through_a_real_shell() {
        let tricky = "a b'c\"d$HOME`x`\\e;f";
        let out = render(&AppType::Claude, &claude_provider(tricky), EnvShell::Bash).unwrap();
        let script = format!("{out}printf '%s' \"$ANTHROPIC_AUTH_TOKEN\"");
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .env_remove("ANTHROPIC_AUTH_TOKEN")
            .output()
            .expect("run sh");
        assert_eq!(String::from_utf8_lossy(&output.stdout), tricky);
    }

    #[test]
    fn other_shells_escape_their_quote_characters() {
        let vars = vec![("K".to_string(), "it's".to_string())];
        assert_eq!(render_vars(&vars, EnvShell::Fish), "set -gx K 'it\\'s'\n");
        assert_eq!(
            render_vars(&vars, EnvShell::PowerShell),
            "$env:K = 'it''s'\n"
        );
        assert_eq!(render_vars(&vars, EnvShell::Dotenv), "K=\"it's\"\n");
        let plain = vec![("K".to_string(), "v".to_string())];
        assert_eq!(render_vars(&plain, EnvShell::Dotenv), "K='v'\n");
    }

    #[test]
    fn additive_apps_are_rejected() {
        assert!(render(&AppType::OpenCode, &claude_provider("x"), EnvShell::Bash).is_err());
    }
}
