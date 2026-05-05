use super::common::MAX_DIR_ENTRIES;
use crate::mcp::CodepalServer;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::fs;

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct LsDirParams {
    #[schemars(description = "Path of directory to list")]
    pub path: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct LsDirResult {
    #[schemars(description = "Directory listing")]
    pub entries: Vec<String>,
}

pub async fn ls(
    server: &CodepalServer,
    params: LsDirParams,
) -> Result<LsDirResult, rmcp::ErrorData> {
    eprintln!("tool: ls(path={})", params.path);
    let path = server
        .path_check_allowed(params.path.into())
        .await
        .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
    let meta = fs::metadata(&path).await.map_err(|_| {
        rmcp::ErrorData::invalid_params(format!("`{}`: EPERM", path.display()), None)
    })?;
    if !meta.is_dir() {
        return Err(rmcp::ErrorData::invalid_params(
            format!("`{}`: Not a directory", path.display()),
            None,
        ));
    }

    let mut read_dir = fs::read_dir(&path).await.map_err(|_| {
        rmcp::ErrorData::invalid_params(format!("`{}`: EPERM", path.display()), None)
    })?;

    let mut entries = Vec::with_capacity(256);
    let mut truncated = false;
    while let Some(entry) = read_dir.next_entry().await.map_err(|_| {
        rmcp::ErrorData::invalid_params(format!("`{}`: EPERM", path.display()), None)
    })? {
        if entries.len() >= MAX_DIR_ENTRIES {
            truncated = true;
            break;
        }
        if let Ok(file_type) = entry.file_type().await {
            let suffix = if file_type.is_dir() { "/" } else { "" };
            entries.push(format!("{}{suffix}", entry.file_name().to_string_lossy()));
        }
    }
    entries.sort();
    if truncated {
        entries.push("... more than maximum number of entries ...".to_string());
    }
    Ok(LsDirResult { entries })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::handler::server::wrapper::Parameters;
    use tokio::fs;

    #[tokio::test]
    async fn ls_returns_sorted_entries() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("b.txt"), "").await.unwrap();
        fs::write(dir.path().join("a.txt"), "").await.unwrap();
        fs::create_dir(dir.path().join("subdir")).await.unwrap();

        let server = crate::mcp::make_test_server(dir.path()).await;
        let result = server
            .ls(Parameters(LsDirParams {
                path: dir.path().to_str().unwrap().to_string(),
            }))
            .await
            .unwrap();
        assert_eq!(result.0.entries, vec!["a.txt", "b.txt", "subdir/"]);
    }

    #[tokio::test]
    async fn ls_on_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        fs::write(&file, "hi").await.unwrap();

        let server = crate::mcp::make_test_server(dir.path()).await;
        let err = server
            .ls(Parameters(LsDirParams {
                path: file.to_str().unwrap().to_string(),
            }))
            .await
            .err()
            .expect("expected Err");
        assert!(err.message.contains("Not a directory"));
    }

    #[tokio::test]
    async fn ls_outside_allowlist_errors() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();

        let server = crate::mcp::make_test_server(dir.path()).await;
        let err = server
            .ls(Parameters(LsDirParams {
                path: other.path().to_str().unwrap().to_string(),
            }))
            .await
            .err()
            .expect("expected Err");
        assert!(err.message.contains("EPERM"));
    }
}
