use super::common::{create_mem_tables, prune_unreferenced_values};
use crate::mcp::{
    CodepalServer,
    structs::{MemoryDeleteParams, MemoryDeleteResult},
};
use rusqlite as sql;

pub async fn mem_delete(
    server: &CodepalServer,
    params: MemoryDeleteParams,
) -> Result<MemoryDeleteResult, rmcp::ErrorData> {
    let mut guard = server.mem_db_conn.lock().expect("Lock poisoned");
    if guard.is_none() {
        if !server.mem_db_path.exists() {
            return Ok(MemoryDeleteResult { found: false });
        }
        let conn = sql::Connection::open(&server.mem_db_path)
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        create_mem_tables(&conn)
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        *guard = Some(conn);
    }
    let conn = guard.as_mut().unwrap();
    let n = conn
        .execute(
            "DELETE FROM memory WHERE key = ?1",
            sql::params![params.key],
        )
        .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
    prune_unreferenced_values(conn)
        .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
    Ok(MemoryDeleteResult { found: n > 0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Opts;
    use crate::mcp::structs::{MemoryLoadParams, MemoryStoreParams};
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
    async fn mem_delete_existing_key() {
        let dir = tempfile::tempdir().unwrap();
        let server = make_server(dir.path()).await;

        server
            .mem_store(Parameters(MemoryStoreParams {
                keys: vec!["mykey".to_string()],
                value: "myvalue".to_string(),
            }))
            .await
            .unwrap();

        let del_result = server
            .mem_delete(Parameters(MemoryDeleteParams {
                key: "mykey".to_string(),
            }))
            .await
            .unwrap();
        assert!(del_result.0.found);

        let load_result = server
            .mem_load(Parameters(MemoryLoadParams {
                keys: vec!["mykey".to_string()],
            }))
            .await
            .unwrap();
        assert_eq!(load_result.0.value, None);
    }

    #[tokio::test]
    async fn mem_delete_nonexistent_key() {
        let dir = tempfile::tempdir().unwrap();
        let server = make_server(dir.path()).await;

        let result = server
            .mem_delete(Parameters(MemoryDeleteParams {
                key: "ghost".to_string(),
            }))
            .await
            .unwrap();
        assert!(!result.0.found);
    }
}
