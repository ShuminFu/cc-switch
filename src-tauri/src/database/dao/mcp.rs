//! MCP 服务器数据访问对象
//!
//! 提供 MCP 服务器的 CRUD 操作。应用启用状态存于关联表 `mcp_server_apps`
//! （见 [`super::app_sets`]），本模块不按应用命名任何列。

use super::app_sets::MCP_SERVER_APPS;
use crate::app_config::McpServer;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use indexmap::IndexMap;
use rusqlite::params;

impl Database {
    /// 获取所有 MCP 服务器
    pub fn get_all_mcp_servers(&self) -> Result<IndexMap<String, McpServer>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut app_sets = MCP_SERVER_APPS.read_all(&conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, name, server_config, description, homepage, docs, tags
                 FROM mcp_servers
                 ORDER BY name ASC, id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let server_iter = stmt
            .query_map([], |row| {
                let server_config_str: String = row.get("server_config")?;
                let tags_str: String = row.get("tags")?;
                Ok(McpServer {
                    id: row.get("id")?,
                    name: row.get("name")?,
                    server: serde_json::from_str(&server_config_str).unwrap_or_default(),
                    apps: Default::default(),
                    description: row.get("description")?,
                    homepage: row.get("homepage")?,
                    docs: row.get("docs")?,
                    tags: serde_json::from_str(&tags_str).unwrap_or_default(),
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut servers = IndexMap::new();
        for server_res in server_iter {
            let mut server = server_res.map_err(|e| AppError::Database(e.to_string()))?;
            server.apps = app_sets.remove(&server.id).unwrap_or_default();
            servers.insert(server.id.clone(), server);
        }
        Ok(servers)
    }

    /// 保存 MCP 服务器
    pub fn save_mcp_server(&self, server: &McpServer) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .savepoint()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Self::save_mcp_server_on_conn(&tx, server)?;
        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 在给定连接（通常是事务）上保存 MCP 服务器及其应用启用状态
    pub(crate) fn save_mcp_server_on_conn(
        conn: &rusqlite::Connection,
        server: &McpServer,
    ) -> Result<(), AppError> {
        conn.execute(
            "INSERT OR REPLACE INTO mcp_servers (
                id, name, server_config, description, homepage, docs, tags
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                server.id,
                server.name,
                serde_json::to_string(&server.server).map_err(|e| AppError::Database(format!(
                    "Failed to serialize server config: {e}"
                )))?,
                server.description,
                server.homepage,
                server.docs,
                serde_json::to_string(&server.tags)
                    .map_err(|e| AppError::Database(format!("Failed to serialize tags: {e}")))?,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        MCP_SERVER_APPS.write(conn, &server.id, &server.apps)
    }

    /// 删除 MCP 服务器
    pub fn delete_mcp_server(&self, id: &str) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .savepoint()
            .map_err(|e| AppError::Database(e.to_string()))?;
        tx.execute("DELETE FROM mcp_servers WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        MCP_SERVER_APPS.delete(&tx, id)?;
        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }
}
