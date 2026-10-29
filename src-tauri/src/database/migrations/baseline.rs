//! v1 冻结基线
//!
//! 建表语句是 v3.8.3（schema v1）发布时的原样（与 `tests/fixtures/db/v1.sql` 一致），
//! **永远不要修改**：之后的每个 schema 变化都必须作为新的迁移步骤追加。
//!
//! - 全新数据库：创建这 7 张 v1 表，之后由 v2+ 步骤演进到当前结构。
//! - v0 数据库（尚未设置 user_version 的早期版本）：表已存在时 `IF NOT EXISTS` 跳过，
//!   再补齐 v1 新增的列。

use crate::database::Database;
use crate::error::AppError;
use rusqlite::Connection;

const V1_TABLES: &str = "
    CREATE TABLE IF NOT EXISTS providers (
        id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        name TEXT NOT NULL,
        settings_config TEXT NOT NULL,
        website_url TEXT,
        category TEXT,
        created_at INTEGER,
        sort_index INTEGER,
        notes TEXT,
        icon TEXT,
        icon_color TEXT,
        meta TEXT NOT NULL DEFAULT '{}',
        is_current BOOLEAN NOT NULL DEFAULT 0,
        PRIMARY KEY (id, app_type)
    );
    CREATE TABLE IF NOT EXISTS provider_endpoints (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        provider_id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        url TEXT NOT NULL,
        added_at INTEGER,
        FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type) ON DELETE CASCADE
    );
    CREATE TABLE IF NOT EXISTS mcp_servers (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        server_config TEXT NOT NULL,
        description TEXT,
        homepage TEXT,
        docs TEXT,
        tags TEXT NOT NULL DEFAULT '[]',
        enabled_claude BOOLEAN NOT NULL DEFAULT 0,
        enabled_codex BOOLEAN NOT NULL DEFAULT 0,
        enabled_gemini BOOLEAN NOT NULL DEFAULT 0
    );
    CREATE TABLE IF NOT EXISTS prompts (
        id TEXT NOT NULL,
        app_type TEXT NOT NULL,
        name TEXT NOT NULL,
        content TEXT NOT NULL,
        description TEXT,
        enabled BOOLEAN NOT NULL DEFAULT 1,
        created_at INTEGER,
        updated_at INTEGER,
        PRIMARY KEY (id, app_type)
    );
    CREATE TABLE IF NOT EXISTS skills (
        key TEXT PRIMARY KEY,
        installed BOOLEAN NOT NULL DEFAULT 0,
        installed_at INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE IF NOT EXISTS skill_repos (
        owner TEXT NOT NULL,
        name TEXT NOT NULL,
        branch TEXT NOT NULL DEFAULT 'main',
        enabled BOOLEAN NOT NULL DEFAULT 1,
        PRIMARY KEY (owner, name)
    );
    CREATE TABLE IF NOT EXISTS settings (
        key TEXT PRIMARY KEY,
        value TEXT
    );
";

pub(super) fn apply(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(V1_TABLES)
        .map_err(|e| AppError::Database(format!("创建 v1 基线表失败: {e}")))?;
    top_up_v0_columns(conn)
}

/// v0 -> v1：补齐早期数据库缺失的列
fn top_up_v0_columns(conn: &Connection) -> Result<(), AppError> {
    // providers 表
    Database::add_column_if_missing(conn, "providers", "category", "TEXT")?;
    Database::add_column_if_missing(conn, "providers", "created_at", "INTEGER")?;
    Database::add_column_if_missing(conn, "providers", "sort_index", "INTEGER")?;
    Database::add_column_if_missing(conn, "providers", "notes", "TEXT")?;
    Database::add_column_if_missing(conn, "providers", "icon", "TEXT")?;
    Database::add_column_if_missing(conn, "providers", "icon_color", "TEXT")?;
    Database::add_column_if_missing(conn, "providers", "meta", "TEXT NOT NULL DEFAULT '{}'")?;
    Database::add_column_if_missing(
        conn,
        "providers",
        "is_current",
        "BOOLEAN NOT NULL DEFAULT 0",
    )?;

    // provider_endpoints 表
    Database::add_column_if_missing(conn, "provider_endpoints", "added_at", "INTEGER")?;

    // mcp_servers 表
    Database::add_column_if_missing(conn, "mcp_servers", "description", "TEXT")?;
    Database::add_column_if_missing(conn, "mcp_servers", "homepage", "TEXT")?;
    Database::add_column_if_missing(conn, "mcp_servers", "docs", "TEXT")?;
    Database::add_column_if_missing(conn, "mcp_servers", "tags", "TEXT NOT NULL DEFAULT '[]'")?;
    Database::add_column_if_missing(
        conn,
        "mcp_servers",
        "enabled_codex",
        "BOOLEAN NOT NULL DEFAULT 0",
    )?;
    Database::add_column_if_missing(
        conn,
        "mcp_servers",
        "enabled_gemini",
        "BOOLEAN NOT NULL DEFAULT 0",
    )?;

    // prompts 表
    Database::add_column_if_missing(conn, "prompts", "description", "TEXT")?;
    Database::add_column_if_missing(conn, "prompts", "enabled", "BOOLEAN NOT NULL DEFAULT 1")?;
    Database::add_column_if_missing(conn, "prompts", "created_at", "INTEGER")?;
    Database::add_column_if_missing(conn, "prompts", "updated_at", "INTEGER")?;

    // skills 表
    Database::add_column_if_missing(conn, "skills", "installed_at", "INTEGER NOT NULL DEFAULT 0")?;

    // skill_repos 表
    Database::add_column_if_missing(
        conn,
        "skill_repos",
        "branch",
        "TEXT NOT NULL DEFAULT 'main'",
    )?;
    Database::add_column_if_missing(conn, "skill_repos", "enabled", "BOOLEAN NOT NULL DEFAULT 1")?;
    // 注意: skills_path 字段已被移除，因为现在支持全仓库递归扫描

    Ok(())
}
