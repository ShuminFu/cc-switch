//! Session export (PRODUCT-48): save a formatted transcript to a file the user
//! picks in a native save dialog. The webview supplies only the content and a
//! suggested file name, never the destination path.

use std::path::{Path, PathBuf};

use tauri_plugin_dialog::DialogExt;

/// Upper bound on one export, so a runaway caller cannot fill the disk.
const MAX_EXPORT_BYTES: usize = 64 * 1024 * 1024;

fn extension_for(format: &str) -> Result<&'static str, String> {
    match format {
        "markdown" => Ok("md"),
        "json" => Ok("json"),
        other => Err(format!("Unsupported export format: {other}")),
    }
}

/// Reduce a suggested name to a plain file name with the right extension.
fn sanitize_file_name(default_name: &str, ext: &str) -> String {
    let base = Path::new(default_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect::<String>();
    let base = base.trim().trim_start_matches('.');
    let stem = Path::new(base)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("session");
    format!("{stem}.{ext}")
}

/// Ensure the chosen path keeps the export's extension.
fn with_extension(path: PathBuf, ext: &str) -> PathBuf {
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case(ext) => path,
        _ => {
            let mut name = path.as_os_str().to_os_string();
            name.push(".");
            name.push(ext);
            PathBuf::from(name)
        }
    }
}

fn write_export(path: &Path, content: &str) -> Result<(), String> {
    crate::config::write_text_file(path, content).map_err(|e| e.to_string())
}

/// Show a save dialog and write `content` there. Returns the saved path, or
/// `None` when the user cancelled.
#[tauri::command]
pub async fn save_session_export<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    #[allow(non_snake_case)] defaultName: String,
    format: String,
    content: String,
) -> Result<Option<String>, String> {
    let ext = extension_for(&format)?;
    if content.len() > MAX_EXPORT_BYTES {
        return Err(format!(
            "Export is too large ({} bytes, max {MAX_EXPORT_BYTES})",
            content.len()
        ));
    }
    let filter_name = if ext == "md" { "Markdown" } else { "JSON" };
    let picked = app
        .dialog()
        .file()
        .add_filter(filter_name, &[ext])
        .set_file_name(sanitize_file_name(&defaultName, ext))
        .blocking_save_file();
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    let path = with_extension(path, ext);
    write_export(&path, &content)?;
    Ok(Some(path.to_string_lossy().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_formats_are_accepted() {
        assert_eq!(extension_for("markdown").unwrap(), "md");
        assert_eq!(extension_for("json").unwrap(), "json");
        assert!(extension_for("exe").is_err());
    }

    #[test]
    fn suggested_names_cannot_carry_directories() {
        assert_eq!(sanitize_file_name("../../etc/passwd", "md"), "passwd.md");
        assert_eq!(sanitize_file_name("a:b*c.md", "md"), "a-b-c.md");
        assert_eq!(sanitize_file_name("", "json"), "session.json");
        assert_eq!(sanitize_file_name(".hidden", "json"), "hidden.json");
    }

    #[test]
    fn chosen_path_keeps_the_export_extension() {
        assert_eq!(
            with_extension(PathBuf::from("/tmp/x.md"), "md"),
            PathBuf::from("/tmp/x.md")
        );
        assert_eq!(
            with_extension(PathBuf::from("/tmp/x"), "json"),
            PathBuf::from("/tmp/x.json")
        );
    }

    #[test]
    fn writes_the_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("out.md");
        write_export(&path, "# hi\n").unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "# hi\n");
    }
}
