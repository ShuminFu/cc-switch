//! Usage data export (FE-UI-25 / PRODUCT-50).
//!
//! The dashboard builds the CSV/JSON text from the same `get_request_logs`
//! pages it renders and hands it to this command, which asks for the
//! destination with a native save dialog and writes the file. The webview
//! never chooses the path itself, and only `.csv` / `.json` destinations are
//! accepted, so the command cannot be used to overwrite config files or
//! scripts.

use std::path::{Path, PathBuf};

use tauri_plugin_dialog::DialogExt;

/// Upper bound on one export (a year of heavy proxy traffic is far below this).
const MAX_EXPORT_BYTES: usize = 256 * 1024 * 1024;

fn export_extension(format: &str) -> Result<&'static str, String> {
    match format {
        "csv" => Ok("csv"),
        "json" => Ok("json"),
        other => Err(format!("Unsupported usage export format: {other}")),
    }
}

/// Validate the chosen destination; appends the extension when the user
/// typed a name without one.
fn resolve_export_target(path: PathBuf, ext: &str, content_len: usize) -> Result<PathBuf, String> {
    if content_len > MAX_EXPORT_BYTES {
        return Err(format!(
            "Usage export is too large ({content_len} bytes, limit {MAX_EXPORT_BYTES})"
        ));
    }
    let path = if path.extension().is_none() {
        path.with_extension(ext)
    } else {
        path
    };
    let actual = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match actual.as_deref() {
        Some("csv") | Some("json") => {}
        _ => return Err("Usage exports must be saved as a .csv or .json file".to_string()),
    }
    if path.is_dir() {
        return Err(format!("{} is a directory", path.display()));
    }
    Ok(path)
}

fn write_export(path: &Path, content: &str) -> Result<(), String> {
    crate::config::write_text_file(path, content).map_err(|e| e.to_string())
}

/// Ask where to save an exported usage report (CSV or JSON text) and write it.
/// Returns the saved path, or `None` when the user cancelled the dialog.
#[tauri::command]
pub async fn save_usage_export<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    #[allow(non_snake_case)] defaultName: String,
    format: String,
    content: String,
) -> Result<Option<String>, String> {
    let ext = export_extension(&format)?;
    tauri::async_runtime::spawn_blocking(move || {
        let chosen = app
            .dialog()
            .file()
            .add_filter(ext.to_ascii_uppercase(), &[ext])
            .set_file_name(&defaultName)
            .blocking_save_file();
        let Some(chosen) = chosen else {
            return Ok(None);
        };
        let path = chosen
            .into_path()
            .map_err(|e| format!("Invalid export path: {e}"))?;
        let path = resolve_export_target(path, ext, content.len())?;
        write_export(&path, &content)?;
        Ok(Some(path.to_string_lossy().to_string()))
    })
    .await
    .map_err(|e| format!("Export failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_csv_and_json_formats() {
        assert_eq!(export_extension("csv"), Ok("csv"));
        assert_eq!(export_extension("json"), Ok("json"));
        assert!(export_extension("sh").is_err());
    }

    #[test]
    fn accepts_csv_and_json_targets_only() {
        let dir = tempfile::tempdir().unwrap();
        let ok = |name: &str| resolve_export_target(dir.path().join(name), "csv", 10);
        assert!(ok("usage.csv").is_ok());
        assert!(ok("usage.JSON").is_ok());
        assert_eq!(ok("usage").unwrap(), dir.path().join("usage.csv"));
        // A dotfile name has no extension, so it becomes `.bashrc.csv`.
        assert_eq!(ok(".bashrc").unwrap(), dir.path().join(".bashrc.csv"));
        assert!(ok("run.sh").is_err());
        assert!(ok("config.toml").is_err());
    }

    #[test]
    fn rejects_directories_and_oversized_content() {
        let dir = tempfile::tempdir().unwrap();
        let as_dir = dir.path().join("folder.csv");
        std::fs::create_dir(&as_dir).unwrap();
        assert!(resolve_export_target(as_dir, "csv", 10).is_err());
        assert!(
            resolve_export_target(dir.path().join("big.csv"), "csv", MAX_EXPORT_BYTES + 1).is_err()
        );
    }

    #[test]
    fn writes_the_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.csv");
        write_export(&path, "a,b\r\n1,2\r\n").unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "a,b\r\n1,2\r\n");
    }
}
