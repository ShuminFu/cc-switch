//! Native save/open dialogs for feature exports and imports. The webview
//! never supplies a destination or source path: the user picks it here.

use std::path::{Path, PathBuf};

use tauri_plugin_dialog::DialogExt;

/// Upper bound on one export or import, so a runaway caller cannot fill the
/// disk or memory.
pub const MAX_FILE_BYTES: usize = 32 * 1024 * 1024;

/// Reduce a suggested name to a plain file name.
pub fn sanitize_file_name(default_name: &str, fallback: &str) -> String {
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
    let base = base.trim().trim_start_matches('.').to_string();
    if base.is_empty() {
        fallback.to_string()
    } else {
        base
    }
}

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

/// Ask for a destination and write `content` there (owner-only when
/// `secret`). Returns the saved path or `None` when cancelled.
pub fn save_text<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    default_name: &str,
    filter_name: &str,
    ext: &str,
    content: &str,
) -> Result<Option<String>, String> {
    if content.len() > MAX_FILE_BYTES {
        return Err(format!(
            "Export is too large ({} bytes, max {MAX_FILE_BYTES})",
            content.len()
        ));
    }
    let picked = app
        .dialog()
        .file()
        .add_filter(filter_name, &[ext])
        .set_file_name(sanitize_file_name(default_name, &format!("export.{ext}")))
        .blocking_save_file();
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = with_extension(picked.into_path().map_err(|e| e.to_string())?, ext);
    crate::config::write_text_file(&path, content).map_err(|e| e.to_string())?;
    Ok(Some(path.to_string_lossy().to_string()))
}

/// Ask for a file to open and return its text, or `None` when cancelled.
pub fn open_text<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    filter_name: &str,
    exts: &[&str],
) -> Result<Option<String>, String> {
    let picked = app
        .dialog()
        .file()
        .add_filter(filter_name, exts)
        .blocking_pick_file();
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    let len = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    if len as usize > MAX_FILE_BYTES {
        return Err(format!("File is too large ({len} bytes)"));
    }
    std::fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cannot_carry_directories() {
        assert_eq!(sanitize_file_name("../../x.jsonl", "f"), "x.jsonl");
        assert_eq!(sanitize_file_name("", "f.json"), "f.json");
        assert_eq!(sanitize_file_name(".hidden", "f"), "hidden");
        assert_eq!(
            with_extension(PathBuf::from("/tmp/a"), "json"),
            PathBuf::from("/tmp/a.json")
        );
    }
}
