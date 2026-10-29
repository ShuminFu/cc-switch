//! 连接级 PRAGMA：journal_mode / synchronous / busy_timeout
//!
//! 本地磁盘上的数据库使用 WAL + `synchronous=NORMAL`，让代理日志写入、会话同步与 UI 查询
//! 不再互相阻塞；所有文件库都设置 `busy_timeout`，短暂的锁冲突会等待而不是直接报
//! `database is locked`。
//!
//! 数据目录位于云同步目录（iCloud Drive / Dropbox / OneDrive / Google Drive）或网络文件系统
//! 时保持 DELETE 模式：同步客户端会单独上传 `-wal` / `-shm` 文件，网络文件系统也不支持
//! WAL 依赖的共享内存，两者都可能导致数据库损坏。

use crate::error::AppError;
use rusqlite::Connection;
use std::path::{Component, Path};
use std::time::Duration;

/// 等待其他连接释放锁的最长时间
pub(crate) const BUSY_TIMEOUT: Duration = Duration::from_millis(5000);

/// 为磁盘上的主数据库连接设置 busy_timeout 与 journal 模式。
///
/// `db_dir` 为数据库文件所在目录（必须已存在）。journal 模式切换失败（如其他进程仍持有
/// 连接）只记录日志，不阻止启动。返回最终生效的 journal 模式（小写）。
pub(crate) fn configure_file_connection(
    conn: &Connection,
    db_dir: &Path,
) -> Result<String, AppError> {
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|e| AppError::Database(format!("设置 busy_timeout 失败: {e}")))?;

    let target = match non_local_storage_reason(db_dir) {
        Some(reason) => {
            log::info!(
                "Database directory {} is on {reason}; keeping journal_mode=DELETE",
                db_dir.display()
            );
            "delete"
        }
        None => "wal",
    };

    let mode = match set_journal_mode(conn, target) {
        Ok(mode) => mode,
        Err(e) => {
            log::warn!("Failed to set journal_mode={target}: {e}");
            current_journal_mode(conn)?
        }
    };
    if mode != target {
        log::warn!("Requested journal_mode={target}, SQLite kept journal_mode={mode}");
    }

    if mode == "wal" {
        // WAL 下 NORMAL 不会损坏数据库，只可能在断电时丢失最后几次提交
        conn.execute_batch("PRAGMA synchronous = NORMAL;")
            .map_err(|e| AppError::Database(format!("设置 synchronous 失败: {e}")))?;
    }
    log::info!("Database journal_mode={mode}");
    Ok(mode)
}

/// 设置 journal 模式并返回 SQLite 实际采用的模式（小写）
pub(crate) fn set_journal_mode(conn: &Connection, mode: &str) -> Result<String, AppError> {
    conn.query_row(&format!("PRAGMA journal_mode = {mode};"), [], |row| {
        row.get::<_, String>(0)
    })
    .map(|m| m.to_ascii_lowercase())
    .map_err(|e| AppError::Database(format!("设置 journal_mode 失败: {e}")))
}

pub(crate) fn current_journal_mode(conn: &Connection) -> Result<String, AppError> {
    conn.query_row("PRAGMA journal_mode;", [], |row| row.get::<_, String>(0))
        .map(|m| m.to_ascii_lowercase())
        .map_err(|e| AppError::Database(format!("读取 journal_mode 失败: {e}")))
}

/// 目录不适合 WAL 时返回原因（云同步目录或网络文件系统），本地目录返回 None
pub(crate) fn non_local_storage_reason(dir: &Path) -> Option<String> {
    if let Some(provider) = cloud_sync_root(dir) {
        return Some(format!("a {provider} sync folder"));
    }
    network_filesystem(dir).map(|fs| format!("a network filesystem ({fs})"))
}

/// 路径是否位于已知云同步根目录下，返回服务名
pub(crate) fn cloud_sync_root(dir: &Path) -> Option<&'static str> {
    let names: Vec<String> = dir
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect();

    names.iter().enumerate().find_map(|(i, name)| {
        let name = name.as_str();
        let parent = i.checked_sub(1).map(|p| names[p].as_str());
        match name {
            // macOS: ~/Library/Mobile Documents/com~apple~CloudDocs；Windows: %USERPROFILE%\iCloudDrive
            "mobile documents" | "com~apple~clouddocs" | "icloud drive" | "iclouddrive" => {
                Some("iCloud Drive")
            }
            // macOS File Provider：~/Library/CloudStorage/<Dropbox|OneDrive-xxx|GoogleDrive-xxx|Box>
            "cloudstorage" if parent == Some("library") => Some("cloud storage"),
            "dropbox" => Some("Dropbox"),
            // 「OneDrive」「OneDrive - Contoso」「OneDrive-Personal」
            "onedrive" => Some("OneDrive"),
            "google drive" | "googledrive" | "my drive" => Some("Google Drive"),
            // 「Dropbox (Personal)」「Dropbox (Contoso)」
            _ if name.starts_with("dropbox (") => Some("Dropbox"),
            _ if name.starts_with("onedrive - ") || name.starts_with("onedrive-") => {
                Some("OneDrive")
            }
            _ if name.starts_with("googledrive-") => Some("Google Drive"),
            _ => None,
        }
    })
}

/// statfs 报告的网络文件系统类型（Linux）
#[cfg(target_os = "linux")]
fn network_filesystem(dir: &Path) -> Option<String> {
    use std::os::unix::ffi::OsStrExt;

    const NETWORK_FS_MAGIC: &[(u32, &str)] = &[
        (0x6969, "nfs"),
        (0x517B, "smb"),
        (0xFF53_4D42, "cifs"),
        (0xFE53_4D42, "smb2"),
        (0x564C, "ncp"),
        (0x7375_7245, "coda"),
        (0x5346_414F, "afs"),
        (0x6B41_4653, "afs"),
        (0x0102_1997, "9p"),
        (0x00C3_6400, "ceph"),
        (0x0BD0_0BD0, "lustre"),
        (0x4750_4653, "gpfs"),
    ];

    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` 是以 NUL 结尾的 C 字符串，`stat` 是可写的 statfs 结构体
    if unsafe { libc::statfs(path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    // f_type 的宽度随架构不同（i32/i64），魔数都在 32 位以内
    let magic = stat.f_type as u32;
    NETWORK_FS_MAGIC
        .iter()
        .find(|(m, _)| *m == magic)
        .map(|(_, name)| (*name).to_string())
}

/// statfs 未带 MNT_LOCAL 标志的挂载（nfs / smbfs / afpfs / webdav 等，macOS）
#[cfg(target_os = "macos")]
fn network_filesystem(dir: &Path) -> Option<String> {
    use std::os::unix::ffi::OsStrExt;

    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` 是以 NUL 结尾的 C 字符串，`stat` 是可写的 statfs 结构体
    if unsafe { libc::statfs(path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    if stat.f_flags & (libc::MNT_LOCAL as u32) != 0 {
        return None;
    }
    // SAFETY: 内核保证 f_fstypename 以 NUL 结尾
    let name = unsafe { std::ffi::CStr::from_ptr(stat.f_fstypename.as_ptr()) };
    Some(name.to_string_lossy().into_owned())
}

/// UNC 路径或映射的网络驱动器（Windows）
#[cfg(target_os = "windows")]
fn network_filesystem(dir: &Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Prefix;
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;

    // WindowsProgramming 中的 DRIVE_REMOTE；未启用该 feature，直接定义
    const DRIVE_REMOTE: u32 = 4;

    let Some(Component::Prefix(prefix)) = dir.components().next() else {
        return None;
    };
    let root = match prefix.kind() {
        Prefix::UNC(..) | Prefix::VerbatimUNC(..) => return Some("UNC share".to_string()),
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => format!("{}:\\", letter as char),
        _ => return None,
    };
    let wide: Vec<u16> = std::ffi::OsStr::new(&root)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` 是以 NUL 结尾的 UTF-16 字符串
    let drive_type = unsafe { GetDriveTypeW(wide.as_ptr()) };
    (drive_type == DRIVE_REMOTE).then(|| format!("network drive {root}"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn network_filesystem(_dir: &Path) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn cloud_sync_roots_are_detected() {
        let cases = [
            (
                "/Users/a/Library/Mobile Documents/com~apple~CloudDocs/cc-switch",
                "iCloud Drive",
            ),
            ("C:\\Users\\a\\iCloudDrive\\cc-switch", "iCloud Drive"),
            (
                "/Users/a/Library/CloudStorage/Dropbox/cc-switch",
                "cloud storage",
            ),
            ("/home/a/Dropbox/.cc-switch", "Dropbox"),
            ("/home/a/Dropbox (Personal)/.cc-switch", "Dropbox"),
            ("C:\\Users\\a\\OneDrive\\cc-switch", "OneDrive"),
            ("C:\\Users\\a\\OneDrive - Contoso\\cc-switch", "OneDrive"),
            ("/Users/a/Google Drive/My Drive/cc-switch", "Google Drive"),
            ("G:\\My Drive\\cc-switch", "Google Drive"),
        ];
        for (path, provider) in cases {
            let path = PathBuf::from(path.replace('\\', std::path::MAIN_SEPARATOR_STR));
            assert_eq!(cloud_sync_root(&path), Some(provider), "{}", path.display());
        }
    }

    #[test]
    fn local_paths_are_not_cloud_sync_roots() {
        for path in [
            "/home/a/.cc-switch",
            "/Users/a/Library/Application Support/cc-switch",
            "/srv/dropbox-backup/cc-switch",
            "/home/a/CloudStorage/cc-switch",
            "/home/a/onedrivex/cc-switch",
        ] {
            assert_eq!(cloud_sync_root(Path::new(path)), None, "{path}");
        }
    }

    #[test]
    fn temp_dir_is_local_storage() {
        let dir = tempfile::tempdir().expect("tempdir");
        // CI 的临时目录不应位于云同步目录或网络文件系统上
        assert_eq!(non_local_storage_reason(dir.path()), None);
    }

    #[test]
    fn configure_file_connection_uses_wal_locally_and_delete_in_cloud_dirs() {
        let local = tempfile::tempdir().expect("tempdir");
        let conn = Connection::open(local.path().join("a.db")).expect("open local db");
        assert_eq!(
            configure_file_connection(&conn, local.path()).expect("configure local"),
            "wal"
        );
        assert_eq!(current_journal_mode(&conn).expect("journal_mode"), "wal");
        let sync: i64 = conn
            .query_row("PRAGMA synchronous;", [], |r| r.get(0))
            .expect("synchronous");
        assert_eq!(sync, 1, "WAL databases should use synchronous=NORMAL");
        let timeout: i64 = conn
            .query_row("PRAGMA busy_timeout;", [], |r| r.get(0))
            .expect("busy_timeout");
        assert_eq!(timeout, 5000);
        drop(conn);

        // 曾经是 WAL 的库被移到云同步目录后应切回 DELETE
        let cloud_dir = local.path().join("Dropbox");
        std::fs::create_dir_all(&cloud_dir).expect("create cloud dir");
        std::fs::rename(local.path().join("a.db"), cloud_dir.join("a.db")).expect("move db");
        let conn = Connection::open(cloud_dir.join("a.db")).expect("open cloud db");
        assert_eq!(
            configure_file_connection(&conn, &cloud_dir).expect("configure cloud"),
            "delete"
        );
        let timeout: i64 = conn
            .query_row("PRAGMA busy_timeout;", [], |r| r.get(0))
            .expect("busy_timeout");
        assert_eq!(timeout, 5000);
        drop(conn);
        assert!(!cloud_dir.join("a.db-wal").exists());
        assert!(!cloud_dir.join("a.db-shm").exists());
    }
}
