use crate::mcp::{
    CodepalServer, MAX_FIND_FILES,
    structs::{FindFilesParams, FindFilesResult},
};
use tokio::fs;
use walkdir::WalkDir;

pub async fn find(
    server: &CodepalServer,
    params: FindFilesParams,
) -> Result<FindFilesResult, rmcp::ErrorData> {
    let dir = server
        .path_check_allowed(params.path.into())
        .await
        .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
    let meta = fs::metadata(&dir).await.map_err(|_| {
        rmcp::ErrorData::invalid_params(format!("`{}`: EPERM", dir.display()), None)
    })?;
    if !meta.is_dir() {
        return Err(rmcp::ErrorData::invalid_params(
            format!("`{}`: Not a directory", dir.display()),
            None,
        ));
    }

    let re = regex::RegexBuilder::new(&params.pattern)
        .case_insensitive(params.case_insensitive.unwrap_or(true))
        .build()
        .map_err(|e| rmcp::ErrorData::invalid_params(format!("Invalid regex: {e}"), None))?;

    let dir_clone = dir.clone();
    let mut files: Vec<String> = tokio::task::spawn_blocking(move || {
        WalkDir::new(&dir_clone)
            .follow_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| {
                let rel = e.path().strip_prefix(&dir_clone).ok()?;
                let rel_str = rel.to_string_lossy();
                if re.is_match(&rel_str) {
                    Some(e.path().to_string_lossy().into_owned())
                } else {
                    None
                }
            })
            .take(MAX_FIND_FILES + 1)
            .collect()
    })
    .await
    .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;

    if files.len() > MAX_FIND_FILES {
        files.truncate(MAX_FIND_FILES);
        files.push(format!(
            "... limit reached ({MAX_FIND_FILES} files), refine pattern ..."
        ));
    }

    Ok(FindFilesResult { files })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Opts;
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
    async fn find_basic() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("main.rs"), "").await.unwrap();
        fs::write(dir.path().join("lib.rs"), "").await.unwrap();
        fs::write(dir.path().join("readme.txt"), "").await.unwrap();

        let server = make_server(dir.path()).await;
        let result = server
            .find(Parameters(FindFilesParams {
                path: dir.path().to_str().unwrap().to_string(),
                pattern: r"\.rs$".to_string(),
                case_insensitive: Some(false),
            }))
            .await
            .unwrap();
        assert_eq!(result.0.files.len(), 2);
        assert!(result.0.files.iter().any(|f| f.contains("main.rs")));
        assert!(result.0.files.iter().any(|f| f.contains("lib.rs")));
        assert!(!result.0.files.iter().any(|f| f.contains("readme.txt")));
    }

    #[tokio::test]
    async fn find_case_insensitive_default() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Main.RS"), "").await.unwrap();

        let server = make_server(dir.path()).await;
        let result = server
            .find(Parameters(FindFilesParams {
                path: dir.path().to_str().unwrap().to_string(),
                pattern: r"\.rs$".to_string(),
                case_insensitive: None,
            }))
            .await
            .unwrap();
        assert_eq!(result.0.files.len(), 1);
    }

    #[tokio::test]
    async fn find_no_match() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("main.rs"), "").await.unwrap();

        let server = make_server(dir.path()).await;
        let result = server
            .find(Parameters(FindFilesParams {
                path: dir.path().to_str().unwrap().to_string(),
                pattern: r"\.py$".to_string(),
                case_insensitive: None,
            }))
            .await
            .unwrap();
        assert!(result.0.files.is_empty());
    }

    #[tokio::test]
    async fn find_invalid_regex_errors() {
        let dir = tempfile::tempdir().unwrap();

        let server = make_server(dir.path()).await;
        let err = server
            .find(Parameters(FindFilesParams {
                path: dir.path().to_str().unwrap().to_string(),
                pattern: "[invalid".to_string(),
                case_insensitive: None,
            }))
            .await
            .err()
            .expect("expected Err");
        assert!(err.message.contains("Invalid regex"));
    }

    #[tokio::test]
    async fn find_on_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        fs::write(&file, "").await.unwrap();

        let server = make_server(dir.path()).await;
        let err = server
            .find(Parameters(FindFilesParams {
                path: file.to_str().unwrap().to_string(),
                pattern: ".*".to_string(),
                case_insensitive: None,
            }))
            .await
            .err()
            .expect("expected Err");
        assert!(err.message.contains("Not a directory"));
    }

    #[tokio::test]
    async fn find_outside_allowlist_errors() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();

        let server = make_server(dir.path()).await;
        let err = server
            .find(Parameters(FindFilesParams {
                path: other.path().to_str().unwrap().to_string(),
                pattern: ".*".to_string(),
                case_insensitive: None,
            }))
            .await
            .err()
            .expect("expected Err");
        assert!(err.message.contains("EPERM"));
    }
}
