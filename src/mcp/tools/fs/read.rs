use super::common::READ_FILE_MAX_SIZE;
use crate::mcp::{CodepalServer, structs::ReadFileParams};
use tokio::fs;

pub async fn read(
    server: &CodepalServer,
    params: ReadFileParams,
) -> Result<String, rmcp::ErrorData> {
    let path = server
        .path_check_allowed(params.path.into())
        .await
        .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
    let meta = fs::metadata(&path).await.map_err(|_| {
        rmcp::ErrorData::invalid_params(format!("`{}`: EPERM", path.display()), None)
    })?;
    if !meta.is_file() {
        return Err(rmcp::ErrorData::invalid_params(
            format!("`{}`: Not a file", path.display()),
            None,
        ));
    }
    if meta.len() > READ_FILE_MAX_SIZE {
        return Err(rmcp::ErrorData::invalid_params(
            format!("`{}`: File too large", path.display()),
            None,
        ));
    }

    let content = fs::read_to_string(&path).await.map_err(|e| {
        rmcp::ErrorData::invalid_params(format!("`{}`: Read error: {e}", path.display()), None)
    })?;

    let lines: Vec<&str> = content.lines().collect();

    let start: usize = if let Some(start_line) = params.start_line {
        start_line
            .saturating_sub(1)
            .try_into()
            .map_err(|_| rmcp::ErrorData::invalid_params("Invalid line number start", None))?
    } else {
        0
    };
    let end: usize = if let Some(end_line) = params.end_line {
        end_line
            .try_into()
            .map_err(|_| rmcp::ErrorData::invalid_params("Invalid line number end", None))?
    } else {
        lines.len()
    };

    let end = end.min(lines.len());
    let start = start.min(end);

    Ok(lines[start..end].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Opts;
    use rmcp::handler::server::wrapper::Parameters;
    use std::path::Path;
    use tokio::fs;

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
    async fn read_entire_content() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        fs::write(&file, "line1\nline2\nline3\n").await.unwrap();

        let server = make_server(dir.path()).await;
        let content = server
            .read(Parameters(ReadFileParams {
                path: file.to_str().unwrap().to_string(),
                start_line: None,
                end_line: None,
            }))
            .await
            .unwrap();
        assert_eq!(content, "line1\nline2\nline3");
    }

    #[tokio::test]
    async fn read_with_start_line() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        fs::write(&file, "line1\nline2\nline3\n").await.unwrap();

        let server = make_server(dir.path()).await;
        let content = server
            .read(Parameters(ReadFileParams {
                path: file.to_str().unwrap().to_string(),
                start_line: Some(2),
                end_line: None,
            }))
            .await
            .unwrap();
        assert_eq!(content, "line2\nline3");
    }

    #[tokio::test]
    async fn read_with_end_line() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        fs::write(&file, "line1\nline2\nline3\n").await.unwrap();

        let server = make_server(dir.path()).await;
        let content = server
            .read(Parameters(ReadFileParams {
                path: file.to_str().unwrap().to_string(),
                start_line: None,
                end_line: Some(2),
            }))
            .await
            .unwrap();
        assert_eq!(content, "line1\nline2");
    }

    #[tokio::test]
    async fn read_with_line_range() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        fs::write(&file, "line1\nline2\nline3\nline4\n")
            .await
            .unwrap();

        let server = make_server(dir.path()).await;
        let content = server
            .read(Parameters(ReadFileParams {
                path: file.to_str().unwrap().to_string(),
                start_line: Some(2),
                end_line: Some(3),
            }))
            .await
            .unwrap();
        assert_eq!(content, "line2\nline3");
    }

    #[tokio::test]
    async fn read_on_directory_errors() {
        let dir = tempfile::tempdir().unwrap();

        let server = make_server(dir.path()).await;
        let err = server
            .read(Parameters(ReadFileParams {
                path: dir.path().to_str().unwrap().to_string(),
                start_line: None,
                end_line: None,
            }))
            .await
            .unwrap_err();
        assert!(err.message.contains("Not a file"));
    }

    #[tokio::test]
    async fn read_too_large_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("big.bin");
        fs::write(&file, vec![b'x'; (READ_FILE_MAX_SIZE + 1) as usize])
            .await
            .unwrap();

        let server = make_server(dir.path()).await;
        let err = server
            .read(Parameters(ReadFileParams {
                path: file.to_str().unwrap().to_string(),
                start_line: None,
                end_line: None,
            }))
            .await
            .unwrap_err();
        assert!(err.message.contains("File too large"));
    }

    #[tokio::test]
    async fn read_outside_allowlist_errors() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let file = other.path().join("secret.txt");
        fs::write(&file, "secret").await.unwrap();

        let server = make_server(dir.path()).await;
        let err = server
            .read(Parameters(ReadFileParams {
                path: file.to_str().unwrap().to_string(),
                start_line: None,
                end_line: None,
            }))
            .await
            .unwrap_err();
        assert!(err.message.contains("EPERM"));
    }
}
