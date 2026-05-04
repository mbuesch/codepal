use super::common::prune_unreferenced_values;
use crate::mcp::CodepalServer;
use rusqlite as sql;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct MemoryDeleteParams {
    #[schemars(description = "Key to delete from the memory store")]
    pub key: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct MemoryDeleteResult {
    #[schemars(description = "Whether the key existed and was deleted")]
    pub found: bool,
}

pub async fn mem_delete(
    server: &CodepalServer,
    params: MemoryDeleteParams,
) -> Result<MemoryDeleteResult, rmcp::ErrorData> {
    server.with_mem_conn_if_exists(MemoryDeleteResult { found: false }, |conn| {
        let n = conn
            .execute(
                "DELETE FROM memory WHERE key = ?1",
                sql::params![params.key],
            )
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        prune_unreferenced_values(conn)
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        Ok(MemoryDeleteResult { found: n > 0 })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::mem::{mem_load::MemoryLoadParams, mem_store::MemoryStoreParams};
    use rmcp::handler::server::wrapper::Parameters;

    #[tokio::test]
    async fn mem_delete_existing_key() {
        let dir = tempfile::tempdir().unwrap();
        let server = crate::mcp::make_test_server(dir.path()).await;

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
        let server = crate::mcp::make_test_server(dir.path()).await;

        let result = server
            .mem_delete(Parameters(MemoryDeleteParams {
                key: "ghost".to_string(),
            }))
            .await
            .unwrap();
        assert!(!result.0.found);
    }
}
