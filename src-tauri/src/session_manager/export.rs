//! Session transcript export (Markdown / JSON).
//!
//! The Session Manager normalizes every tool's history into `SessionMeta` +
//! `SessionMessage`; this module renders that normalized form into a file the
//! user can share or archive.

use super::{SessionMessage, SessionMeta};
use serde::Serialize;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptFormat {
    Markdown,
    Json,
}

impl TranscriptFormat {
    pub fn extension(self) -> &'static str {
        match self {
            TranscriptFormat::Markdown => "md",
            TranscriptFormat::Json => "json",
        }
    }

    pub fn dialog_label(self) -> &'static str {
        match self {
            TranscriptFormat::Markdown => "Markdown",
            TranscriptFormat::Json => "JSON",
        }
    }
}

impl FromStr for TranscriptFormat {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "markdown" | "md" => Ok(TranscriptFormat::Markdown),
            "json" => Ok(TranscriptFormat::Json),
            other => Err(format!("Unsupported transcript format: {other}")),
        }
    }
}

/// Session scanners and the frontend use millisecond timestamps; tolerate
/// second-precision values from older records.
fn format_timestamp(ts: Option<i64>) -> Option<String> {
    use chrono::TimeZone;

    let ts = ts?;
    let millis = if ts.abs() < 100_000_000_000 {
        ts.checked_mul(1000)?
    } else {
        ts
    };
    chrono::Utc.timestamp_millis_opt(millis).single().map(|dt| {
        dt.with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    })
}

pub fn render_transcript(
    meta: &SessionMeta,
    messages: &[SessionMessage],
    format: TranscriptFormat,
) -> Result<String, String> {
    match format {
        TranscriptFormat::Markdown => Ok(render_markdown(meta, messages)),
        TranscriptFormat::Json => {
            #[derive(Serialize)]
            #[serde(rename_all = "camelCase")]
            struct Envelope<'a> {
                session: &'a SessionMeta,
                messages: &'a [SessionMessage],
            }
            serde_json::to_string_pretty(&Envelope {
                session: meta,
                messages,
            })
            .map(|json| format!("{json}\n"))
            .map_err(|e| format!("Failed to serialize transcript: {e}"))
        }
    }
}

fn render_markdown(meta: &SessionMeta, messages: &[SessionMessage]) -> String {
    let title = meta
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or(meta.session_id.as_str());

    let mut out = String::new();
    out.push_str(&format!("# {title}\n\n"));
    out.push_str(&format!("- Provider: {}\n", meta.provider_id));
    out.push_str(&format!("- Session: {}\n", meta.session_id));
    if let Some(dir) = meta.project_dir.as_deref().filter(|d| !d.is_empty()) {
        out.push_str(&format!("- Project: {dir}\n"));
    }
    if let Some(ts) = format_timestamp(meta.created_at) {
        out.push_str(&format!("- Created: {ts}\n"));
    }
    if let Some(ts) = format_timestamp(meta.last_active_at) {
        out.push_str(&format!("- Last active: {ts}\n"));
    }
    if let Some(source) = meta.source_path.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("- Source: {source}\n"));
    }
    out.push_str(&format!("- Messages: {}\n", messages.len()));

    for message in messages {
        out.push_str("\n---\n\n");
        match format_timestamp(message.ts) {
            Some(ts) => out.push_str(&format!("## {} · {ts}\n\n", message.role)),
            None => out.push_str(&format!("## {}\n\n", message.role)),
        }
        out.push_str(message.content.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> SessionMeta {
        SessionMeta {
            provider_id: "codex".to_string(),
            session_id: "sess-123".to_string(),
            title: Some("Fix the build".to_string()),
            summary: None,
            project_dir: Some("/work/app".to_string()),
            created_at: Some(1_700_000_000_000),
            last_active_at: Some(1_700_000_600),
            source_path: Some("/home/me/.codex/sessions/sess-123.jsonl".to_string()),
            resume_command: None,
        }
    }

    fn messages() -> Vec<SessionMessage> {
        vec![
            SessionMessage {
                role: "user".to_string(),
                content: "Why does `cargo test` fail?\n".to_string(),
                ts: Some(1_700_000_000_000),
            },
            SessionMessage {
                role: "assistant".to_string(),
                content: "Because of a missing feature flag.".to_string(),
                ts: None,
            },
        ]
    }

    #[test]
    fn parses_formats_case_insensitively() {
        assert_eq!(
            "markdown".parse::<TranscriptFormat>(),
            Ok(TranscriptFormat::Markdown)
        );
        assert_eq!(
            "MD".parse::<TranscriptFormat>(),
            Ok(TranscriptFormat::Markdown)
        );
        assert_eq!(
            " json ".parse::<TranscriptFormat>(),
            Ok(TranscriptFormat::Json)
        );
        assert!("pdf".parse::<TranscriptFormat>().is_err());
        assert_eq!(TranscriptFormat::Json.extension(), "json");
    }

    #[test]
    fn markdown_includes_metadata_and_every_message() {
        let rendered =
            render_transcript(&meta(), &messages(), TranscriptFormat::Markdown).expect("render");

        assert!(rendered.starts_with("# Fix the build\n"));
        assert!(rendered.contains("- Provider: codex\n"));
        assert!(rendered.contains("- Session: sess-123\n"));
        assert!(rendered.contains("- Project: /work/app\n"));
        assert!(rendered.contains("- Created: "));
        assert!(rendered.contains("- Last active: "));
        assert!(rendered.contains("- Messages: 2\n"));
        assert!(rendered.contains("## user · "));
        assert!(rendered.contains("Why does `cargo test` fail?\n"));
        assert!(rendered.contains("## assistant\n\nBecause of a missing feature flag.\n"));
        assert_eq!(rendered.matches("\n---\n").count(), 2);
    }

    #[test]
    fn markdown_falls_back_to_session_id_without_title() {
        let mut untitled = meta();
        untitled.title = Some("   ".to_string());
        let rendered =
            render_transcript(&untitled, &[], TranscriptFormat::Markdown).expect("render");
        assert!(rendered.starts_with("# sess-123\n"));
        assert!(rendered.contains("- Messages: 0\n"));
    }

    #[test]
    fn json_round_trips_session_and_messages() {
        let rendered =
            render_transcript(&meta(), &messages(), TranscriptFormat::Json).expect("render");
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid json");
        assert_eq!(value["session"]["sessionId"], "sess-123");
        assert_eq!(value["session"]["projectDir"], "/work/app");
        assert_eq!(value["messages"].as_array().map(Vec::len), Some(2));
        assert_eq!(value["messages"][1]["role"], "assistant");
        assert!(rendered.ends_with('\n'));
    }

    #[test]
    fn timestamps_accept_seconds_and_milliseconds() {
        assert!(format_timestamp(Some(1_700_000_000)).is_some());
        assert!(format_timestamp(Some(1_700_000_000_000)).is_some());
        assert_eq!(format_timestamp(None), None);
    }
}
