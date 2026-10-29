//! 记录由后端文件对话框返回给 webview 的路径。
//!
//! 接受文件系统路径的特权命令（导入/导出 SQL、从 ZIP 安装 Skills、设置配置目录覆盖等）
//! 只信任两类路径：落在允许根目录下的路径，或刚由本进程的对话框命令选出的路径。
//! 这样即使 webview 被注入脚本，也无法让后端读写任意位置的文件。

use std::collections::VecDeque;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 每种用途各自保留的最近选择数量上限
const MAX_ENTRIES: usize = 32;

/// 对话框选择的用途；路径只能用于选出它的那类对话框对应的命令
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickPurpose {
    /// `open_file_dialog`：导入 SQL 备份
    SqlImport,
    /// `save_file_dialog`：导出 SQL 备份
    SqlExport,
    /// `open_zip_file_dialog`：从 ZIP 安装 Skills
    SkillZip,
    /// `pick_directory`：选择目录
    Directory,
}

fn registry() -> &'static Mutex<VecDeque<(PickPurpose, PathBuf)>> {
    static REGISTRY: OnceLock<Mutex<VecDeque<(PickPurpose, PathBuf)>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// 登记一条由对话框返回的路径
pub fn register(purpose: PickPurpose, path: &str) {
    let path = PathBuf::from(path);
    let mut guard = match registry().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.retain(|(p, existing)| !(*p == purpose && *existing == path));
    guard.push_back((purpose, path));
    while guard.iter().filter(|(p, _)| *p == purpose).count() > MAX_ENTRIES {
        if let Some(pos) = guard.iter().position(|(p, _)| *p == purpose) {
            guard.remove(pos);
        }
    }
}

/// 该路径是否由指定用途的对话框选出
pub fn is_picked(purpose: PickPurpose, path: &str) -> bool {
    let path = PathBuf::from(path);
    let guard = match registry().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard
        .iter()
        .any(|(p, existing)| *p == purpose && *existing == path)
}

/// 要求路径由指定用途的对话框选出，否则返回错误
pub fn require_picked(purpose: PickPurpose, path: &str) -> Result<(), String> {
    if is_picked(purpose, path) {
        Ok(())
    } else {
        Err(format!(
            "路径未通过文件选择对话框选择，已拒绝: {path} (path was not chosen through the file dialog)"
        ))
    }
}

/// 路径是否为绝对路径、不含 `..` 等跳转组件，且位于 `root` 之下（词法判断，不访问文件系统）
pub fn is_within_root(path: &Path, root: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return false;
    }
    path.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_path_is_only_valid_for_its_purpose() {
        let path = "/tmp/ccs-picked-paths-test/export.sql";
        assert!(!is_picked(PickPurpose::SqlExport, path));
        register(PickPurpose::SqlExport, path);
        assert!(is_picked(PickPurpose::SqlExport, path));
        assert!(require_picked(PickPurpose::SqlExport, path).is_ok());
        assert!(!is_picked(PickPurpose::SqlImport, path));
        assert!(require_picked(PickPurpose::SqlImport, path).is_err());
    }

    #[test]
    fn registry_keeps_only_recent_entries_per_purpose() {
        let first = "/tmp/ccs-picked-paths-test/dir-0".to_string();
        for i in 0..=MAX_ENTRIES {
            register(
                PickPurpose::Directory,
                &format!("/tmp/ccs-picked-paths-test/dir-{i}"),
            );
        }
        assert!(!is_picked(PickPurpose::Directory, &first));
        assert!(is_picked(
            PickPurpose::Directory,
            &format!("/tmp/ccs-picked-paths-test/dir-{MAX_ENTRIES}")
        ));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn within_root_rejects_relative_and_parent_components() {
        let root = Path::new("/home/alice");
        assert!(is_within_root(Path::new("/home/alice/.cc-switch"), root));
        assert!(!is_within_root(Path::new("/home/alice/../bob"), root));
        assert!(!is_within_root(Path::new("home/alice/x"), root));
        assert!(!is_within_root(Path::new("/etc"), root));
        assert!(!is_within_root(Path::new("/home/alicex"), root));
    }
}
