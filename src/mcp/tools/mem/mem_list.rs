use crate::mcp::CodepalServer;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct MemoryListResult {
    #[schemars(description = "All stored keys")]
    pub keys: Vec<String>,
}

pub async fn mem_list(server: &CodepalServer) -> Result<MemoryListResult, rmcp::ErrorData> {
    eprintln!("tool: mem_list()");
    server.with_mem_conn_if_exists(MemoryListResult { keys: vec![] }, |conn| {
        let mut stmt = conn
            .prepare("SELECT key FROM memory ORDER BY accessed_at DESC")
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        let keys = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(MemoryListResult { keys })
    })
}

#[cfg(test)]
mod tests {
    use crate::mcp::tools::mem::mem_store::MemoryStoreParams;
    use rmcp::handler::server::wrapper::Parameters;

    #[tokio::test]
    async fn mem_list_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let server = crate::mcp::make_test_server(dir.path()).await;

        let result = server.mem_list().await.unwrap();
        assert!(result.0.keys.is_empty());
    }

    #[tokio::test]
    async fn mem_list_after_storing() {
        let dir = tempfile::tempdir().unwrap();
        let server = crate::mcp::make_test_server(dir.path()).await;

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
