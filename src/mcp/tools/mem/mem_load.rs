use super::common::{create_mem_tables, prune_expired_entries};
use crate::mcp::{
    CodepalServer,
    structs::{MemoryLoadParams, MemoryLoadResult},
};
use rusqlite::{self as sql, OptionalExtension as _};

pub async fn mem_load(
    server: &CodepalServer,
    params: MemoryLoadParams,
) -> Result<MemoryLoadResult, rmcp::ErrorData> {
    let mut guard = server.mem_db_conn.lock().expect("Lock poisoned");
    if guard.is_none() {
        if !server.mem_db_path.exists() {
            return Ok(MemoryLoadResult { value: None });
        }
        let conn = sql::Connection::open(&server.mem_db_path)
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        create_mem_tables(&conn)
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        *guard = Some(conn);
    }
    let conn = guard.as_mut().unwrap();
    if let Some(max_age_days) = server.mem_max_age_days {
        prune_expired_entries(conn, max_age_days)
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
    }
    let mut value = None;
    for key in &params.keys {
        // Try exact match first to avoid prefix collisions (e.g. "key1" vs "key10").
        let found_key: Option<String> = conn
            .query_row(
                "SELECT m.key FROM memory m WHERE m.key = ?1",
                sql::params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        let found_key = if found_key.is_some() {
            found_key
        } else {
            let escaped = key
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            conn.query_row(
                "SELECT m.key FROM memory m WHERE m.key LIKE '%' || ?1 || '%' ESCAPE '\\' ORDER BY m.accessed_at DESC LIMIT 1",
                sql::params![escaped],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?
        };
        if let Some(ref fk) = found_key {
            conn.execute(
                "UPDATE memory SET accessed_at = datetime('now'), access_count = access_count + 1 WHERE key = ?1",
                sql::params![fk],
            )
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
            value = conn
                .query_row(
                    "SELECT v.value FROM memory m JOIN mem_values v ON m.value_id = v.id WHERE m.key = ?1",
                    sql::params![fk],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
            break;
        }
    }
    Ok(MemoryLoadResult { value })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Opts;
    use crate::mcp::structs::MemoryStoreParams;
    use rmcp::handler::server::wrapper::Parameters;
    use std::path::Path;

    async fn make_server(workspace: &Path) -> CodepalServer {
        let opts = Opts {
            workspace: workspace.to_path_buf(),
            read_path_allow_list: vec![],
            no_auto_path_allow: false,
            enable_compressed: false,
            dump_memory: false,
            memory_max_age_days: None,
        };
        CodepalServer::new(&opts)
            .await
            .expect("server creation failed")
    }

    #[tokio::test]
    async fn mem_load_returns_none_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let server = make_server(dir.path()).await;

        let result = server
            .mem_load(Parameters(MemoryLoadParams {
                keys: vec!["nonexistent".to_string()],
            }))
            .await
            .unwrap();
        assert_eq!(result.0.value, None);
    }

    #[tokio::test]
    async fn mem_load_first_key_wins() {
        let dir = tempfile::tempdir().unwrap();
        let server = make_server(dir.path()).await;

        server
            .mem_store(Parameters(MemoryStoreParams {
                keys: vec!["key-a".to_string()],
                value: "value-a".to_string(),
            }))
            .await
            .unwrap();
        server
            .mem_store(Parameters(MemoryStoreParams {
                keys: vec!["key-b".to_string()],
                value: "value-b".to_string(),
            }))
            .await
            .unwrap();

        let result = server
            .mem_load(Parameters(MemoryLoadParams {
                keys: vec!["key-a".to_string(), "key-b".to_string()],
            }))
            .await
            .unwrap();
        assert_eq!(result.0.value, Some("value-a".to_string()));
    }
}
