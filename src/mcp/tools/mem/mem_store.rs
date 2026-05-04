use super::common::{create_mem_tables, prune_expired_entries, prune_unreferenced_values};
use crate::mcp::{
    CodepalServer, MEMORY_MAX_KEY_LEN, MEMORY_MAX_KEYS, MEMORY_MAX_VALUE_LEN,
    structs::{MemoryStoreParams, MemoryStoreResult},
};
use rusqlite::{self as sql, OptionalExtension as _};

pub async fn mem_store(
    server: &CodepalServer,
    params: MemoryStoreParams,
) -> Result<MemoryStoreResult, rmcp::ErrorData> {
    if params.keys.is_empty() {
        return Err(rmcp::ErrorData::invalid_params(
            "keys must not be empty",
            None,
        ));
    }
    if params.keys.len() > MEMORY_MAX_KEYS {
        return Err(rmcp::ErrorData::invalid_params(
            format!("too many keys (max {MEMORY_MAX_KEYS})"),
            None,
        ));
    }
    for key in &params.keys {
        if key.len() > MEMORY_MAX_KEY_LEN {
            return Err(rmcp::ErrorData::invalid_params(
                format!("key too long (max {MEMORY_MAX_KEY_LEN} bytes)"),
                None,
            ));
        }
    }
    if params.value.len() > MEMORY_MAX_VALUE_LEN {
        return Err(rmcp::ErrorData::invalid_params(
            format!("value too long (max {} bytes)", MEMORY_MAX_VALUE_LEN),
            None,
        ));
    }
    let mut guard = server.mem_db_conn.lock().expect("Lock poisoned");
    if guard.is_none() {
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
    // Insert the value once; get its id.
    conn.execute(
        "INSERT OR IGNORE INTO mem_values (value) VALUES (?1)",
        sql::params![params.value],
    )
    .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
    let value_id: i64 = conn
        .query_row(
            "SELECT id FROM mem_values WHERE value = ?1",
            sql::params![params.value],
            |row| row.get(0),
        )
        .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
    // Map every key to that value id, incrementing access_count on overwrite.
    for key in &params.keys {
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM memory WHERE key = ?1",
                sql::params![key],
                |_| Ok(true),
            )
            .optional()
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?
            .unwrap_or(false);
        if exists {
            eprintln!("mem_store: overwriting existing key `{key}`");
        }
        conn.execute(
            "INSERT INTO memory (key, value_id, stored_at, accessed_at, access_count) \
             VALUES (?1, ?2, datetime('now'), datetime('now'), 1) \
             ON CONFLICT(key) DO UPDATE SET \
                 value_id = excluded.value_id, \
                 stored_at = excluded.stored_at, \
                 accessed_at = datetime('now'), \
                 access_count = memory.access_count + 1",
            sql::params![key, value_id],
        )
        .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
    }
    prune_unreferenced_values(conn)
        .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
    Ok(MemoryStoreResult { success: true })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Opts;
    use crate::mcp::structs::MemoryLoadParams;
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
    async fn mem_store_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let server = make_server(dir.path()).await;

        let store_result = server
            .mem_store(Parameters(MemoryStoreParams {
                keys: vec!["mykey".to_string()],
                value: "myvalue".to_string(),
            }))
            .await
            .unwrap();
        assert!(store_result.0.success);

        let load_result = server
            .mem_load(Parameters(MemoryLoadParams {
                keys: vec!["mykey".to_string()],
            }))
            .await
            .unwrap();
        assert_eq!(load_result.0.value, Some("myvalue".to_string()));
    }

    #[tokio::test]
    async fn mem_store_empty_keys_errors() {
        let dir = tempfile::tempdir().unwrap();
        let server = make_server(dir.path()).await;

        let err = server
            .mem_store(Parameters(MemoryStoreParams {
                keys: vec![],
                value: "value".to_string(),
            }))
            .await
            .err()
            .expect("expected Err");
        assert!(err.message.contains("keys must not be empty"));
    }

    #[tokio::test]
    async fn mem_store_too_many_keys_errors() {
        let dir = tempfile::tempdir().unwrap();
        let server = make_server(dir.path()).await;

        let keys: Vec<String> = (0..=MEMORY_MAX_KEYS).map(|i| format!("key{i}")).collect();
        let err = server
            .mem_store(Parameters(MemoryStoreParams {
                keys,
                value: "value".to_string(),
            }))
            .await
            .err()
            .expect("expected Err");
        assert!(err.message.contains("too many keys"));
    }

    #[tokio::test]
    async fn mem_store_key_too_long_errors() {
        let dir = tempfile::tempdir().unwrap();
        let server = make_server(dir.path()).await;

        let long_key = "k".repeat(MEMORY_MAX_KEY_LEN + 1);
        let err = server
            .mem_store(Parameters(MemoryStoreParams {
                keys: vec![long_key],
                value: "value".to_string(),
            }))
            .await
            .err()
            .expect("expected Err");
        assert!(err.message.contains("key too long"));
    }

    #[tokio::test]
    async fn mem_store_value_too_long_errors() {
        let dir = tempfile::tempdir().unwrap();
        let server = make_server(dir.path()).await;

        let long_value = "v".repeat(MEMORY_MAX_VALUE_LEN + 1);
        let err = server
            .mem_store(Parameters(MemoryStoreParams {
                keys: vec!["mykey".to_string()],
                value: long_value,
            }))
            .await
            .err()
            .expect("expected Err");
        assert!(err.message.contains("value too long"));
    }

    #[tokio::test]
    async fn mem_store_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let server = make_server(dir.path()).await;

        server
            .mem_store(Parameters(MemoryStoreParams {
                keys: vec!["k".to_string()],
                value: "old".to_string(),
            }))
            .await
            .unwrap();
        server
            .mem_store(Parameters(MemoryStoreParams {
                keys: vec!["k".to_string()],
                value: "new".to_string(),
            }))
            .await
            .unwrap();

        let result = server
            .mem_load(Parameters(MemoryLoadParams {
                keys: vec!["k".to_string()],
            }))
            .await
            .unwrap();
        assert_eq!(result.0.value, Some("new".to_string()));
    }

    #[tokio::test]
    async fn mem_store_multi_key_all_loadable() {
        let dir = tempfile::tempdir().unwrap();
        let server = make_server(dir.path()).await;

        server
            .mem_store(Parameters(MemoryStoreParams {
                keys: vec!["alpha".to_string(), "beta".to_string()],
                value: "shared".to_string(),
            }))
            .await
            .unwrap();

        for key in ["alpha", "beta"] {
            let result = server
                .mem_load(Parameters(MemoryLoadParams {
                    keys: vec![key.to_string()],
                }))
                .await
                .unwrap();
            assert_eq!(result.0.value, Some("shared".to_string()), "key={key}");
        }
    }
}
