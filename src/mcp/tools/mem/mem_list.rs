use crate::mcp::{CodepalServer, create_mem_tables, structs::MemoryListResult};
use rusqlite as sql;

pub async fn mem_list(server: &CodepalServer) -> Result<MemoryListResult, rmcp::ErrorData> {
    let mut guard = server.mem_db_conn.lock().expect("Lock poisoned");
    if guard.is_none() {
        if !server.mem_db_path.exists() {
            return Ok(MemoryListResult { keys: vec![] });
        }
        let conn = sql::Connection::open(&server.mem_db_path)
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        create_mem_tables(&conn)
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        *guard = Some(conn);
    }
    let conn = guard.as_mut().unwrap();
    let mut stmt = conn
        .prepare("SELECT key FROM memory ORDER BY accessed_at DESC")
        .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
    let keys: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(MemoryListResult { keys })
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
    async fn mem_list_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let server = make_server(dir.path()).await;

        let result = server.mem_list().await.unwrap();
        assert!(result.0.keys.is_empty());
    }

    #[tokio::test]
    async fn mem_list_after_storing() {
        let dir = tempfile::tempdir().unwrap();
        let server = make_server(dir.path()).await;

        server
            .mem_store(Parameters(MemoryStoreParams {
                keys: vec!["k1".to_string(), "k2".to_string()],
                value: "val".to_string(),
            }))
            .await
            .unwrap();

        let result = server.mem_list().await.unwrap();
        let mut keys = result.0.keys.clone();
        keys.sort();
        assert_eq!(keys, vec!["k1", "k2"]);
    }
}
