use super::common::{
    MAX_GREP_DIR_FILES, MAX_GREP_DIR_MATCHES, MAX_GREP_DIR_RESULT_SIZE, READ_FILE_MAX_SIZE,
    compute_grep_matches, format_grep_ranges,
};
use crate::mcp::CodepalServer;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;
use walkdir::WalkDir;

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct GrepParams {
    #[schemars(
        description = "Path of file or directory to search (directory search is recursive)"
    )]
    pub path: String,
    #[schemars(description = "Regex pattern to search for")]
    pub pattern: String,
    #[schemars(description = "Optional: Case insensitive (default: false)")]
    pub case_insensitive: Option<bool>,
    #[schemars(description = "Optional: Enable `.` matches `\\n` (default: false)")]
    pub dot_matches_newline: Option<bool>,
    #[schemars(description = "Optional: Context lines, like grep -C (default: 0)")]
    pub context_lines: Option<u16>,
}

pub async fn grep(server: &CodepalServer, params: GrepParams) -> Result<String, rmcp::ErrorData> {
    let path = server
        .path_check_allowed(params.path.into())
        .await
        .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
    let meta = fs::metadata(&path).await.map_err(|_| {
        rmcp::ErrorData::invalid_params(format!("`{}`: EPERM", path.display()), None)
    })?;

    let re = regex::RegexBuilder::new(&params.pattern)
        .case_insensitive(params.case_insensitive.unwrap_or(false))
        .dot_matches_new_line(params.dot_matches_newline.unwrap_or(false))
        .build()
        .map_err(|e| rmcp::ErrorData::invalid_params(format!("Invalid regex: {e}"), None))?;

    let ctx = params.context_lines.unwrap_or(0) as usize;

    if meta.is_file() {
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
        let Some((ranges, match_set)) = compute_grep_matches(&lines, &re, ctx) else {
            return Ok(String::new());
        };

        let mut result = String::new();
        format_grep_ranges(&lines, &ranges, &match_set, &mut result, |_, _| false);
        Ok(result)
    } else if meta.is_dir() {
        let dir = path;
        let dir_clone = dir.clone();
        let file_paths: Vec<PathBuf> = tokio::task::spawn_blocking(move || {
            WalkDir::new(&dir_clone)
                .follow_links(false)
                .sort_by_file_name()
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .map(|e| e.into_path())
                .take(MAX_GREP_DIR_FILES)
                .collect()
        })
        .await
        .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;

        let mut result = String::new();
        let mut total_matches: usize = 0;
        let mut limit_reached = false;

        'files: for file_path in &file_paths {
            let meta = match fs::metadata(file_path).await {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.len() > READ_FILE_MAX_SIZE {
                continue;
            }
            let content = match fs::read_to_string(file_path).await {
                Ok(c) => c,
                Err(_) => continue,
            };

            let lines: Vec<&str> = content.lines().collect();
            let Some((ranges, match_set)) = compute_grep_matches(&lines, &re, ctx) else {
                continue;
            };

            result.push_str(&format!("=== {} ===\n", file_path.display()));
            limit_reached = format_grep_ranges(
                &lines,
                &ranges,
                &match_set,
                &mut result,
                |is_match, out_len| {
                    if is_match {
                        total_matches += 1;
                        if total_matches >= MAX_GREP_DIR_MATCHES {
                            return true;
                        }
                    }
                    out_len >= MAX_GREP_DIR_RESULT_SIZE
                },
            );
            if limit_reached {
                break 'files;
            }
        }

        if limit_reached {
            result.push_str(&format!(
                "\n... limit reached ({MAX_GREP_DIR_MATCHES} matches / {MAX_GREP_DIR_FILES} files / {} MB output), refine pattern ...\n",
                MAX_GREP_DIR_RESULT_SIZE / 1024 / 1024,
            ));
        }

        Ok(result)
    } else {
        Err(rmcp::ErrorData::invalid_params(
            format!("`{}`: Not a file or directory", path.display()),
            None,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::handler::server::wrapper::Parameters;
    use tokio::fs;

    #[tokio::test]
    async fn grep_file_basic_match() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("code.txt");
        fs::write(&file, "foo bar\nbaz\nfoo qux\n").await.unwrap();

        let server = crate::mcp::make_test_server(dir.path()).await;
        let result = server
            .grep(Parameters(GrepParams {
                path: file.to_str().unwrap().to_string(),
                pattern: "foo".to_string(),
                case_insensitive: None,
                dot_matches_newline: None,
                context_lines: None,
            }))
            .await
            .unwrap();
        assert!(result.contains("1:foo bar"));
        assert!(result.contains("3:foo qux"));
        assert!(!result.contains("baz"));
    }

    #[tokio::test]
    async fn grep_file_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("code.txt");
        fs::write(&file, "Hello World\nhello world\nGOODBYE\n")
            .await
            .unwrap();

        let server = crate::mcp::make_test_server(dir.path()).await;
        let result = server
            .grep(Parameters(GrepParams {
                path: file.to_str().unwrap().to_string(),
                pattern: "hello".to_string(),
                case_insensitive: Some(true),
                dot_matches_newline: None,
                context_lines: None,
            }))
            .await
            .unwrap();
        assert!(result.contains("Hello World"));
        assert!(result.contains("hello world"));
        assert!(!result.contains("GOODBYE"));
    }

    #[tokio::test]
    async fn grep_file_no_match_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("code.txt");
        fs::write(&file, "foo bar\nbaz\n").await.unwrap();

        let server = crate::mcp::make_test_server(dir.path()).await;
        let result = server
            .grep(Parameters(GrepParams {
                path: file.to_str().unwrap().to_string(),
                pattern: "zzz".to_string(),
                case_insensitive: None,
                dot_matches_newline: None,
                context_lines: None,
            }))
            .await
            .unwrap();
        assert_eq!(result, "");
    }

    #[tokio::test]
    async fn grep_file_context_lines() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("code.txt");
        fs::write(&file, "before\nmatch\nafter\n").await.unwrap();

        let server = crate::mcp::make_test_server(dir.path()).await;
        let result = server
            .grep(Parameters(GrepParams {
                path: file.to_str().unwrap().to_string(),
                pattern: "match".to_string(),
                case_insensitive: None,
                dot_matches_newline: None,
                context_lines: Some(1),
            }))
            .await
            .unwrap();
        assert!(result.contains("1-before"));
        assert!(result.contains("2:match"));
        assert!(result.contains("3-after"));
    }

    #[tokio::test]
    async fn grep_file_invalid_regex_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("code.txt");
        fs::write(&file, "some content\n").await.unwrap();

        let server = crate::mcp::make_test_server(dir.path()).await;
        let err = server
            .grep(Parameters(GrepParams {
                path: file.to_str().unwrap().to_string(),
                pattern: "[invalid".to_string(),
                case_insensitive: None,
                dot_matches_newline: None,
                context_lines: None,
            }))
            .await
            .unwrap_err();
        assert!(err.message.contains("Invalid regex"));
    }

    #[tokio::test]
    async fn grep_file_outside_allowlist_errors() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let file = other.path().join("secret.txt");
        fs::write(&file, "secret").await.unwrap();

        let server = crate::mcp::make_test_server(dir.path()).await;
        let err = server
            .grep(Parameters(GrepParams {
                path: file.to_str().unwrap().to_string(),
                pattern: "secret".to_string(),
                case_insensitive: None,
                dot_matches_newline: None,
                context_lines: None,
            }))
            .await
            .unwrap_err();
        assert!(err.message.contains("EPERM"));
    }

    #[tokio::test]
    async fn grep_dir_basic_match() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "hello world\nfoo bar\n")
            .await
            .unwrap();
        fs::write(dir.path().join("b.txt"), "no match here\n")
            .await
            .unwrap();

        let server = crate::mcp::make_test_server(dir.path()).await;
        let result = server
            .grep(Parameters(GrepParams {
                path: dir.path().to_str().unwrap().to_string(),
                pattern: "hello".to_string(),
                case_insensitive: None,
                dot_matches_newline: None,
                context_lines: None,
            }))
            .await
            .unwrap();
        assert!(result.contains("a.txt"));
        assert!(result.contains("1:hello world"));
        assert!(!result.contains("b.txt"));
    }

    #[tokio::test]
    async fn grep_dir_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "Hello World\n")
            .await
            .unwrap();

        let server = crate::mcp::make_test_server(dir.path()).await;
        let result = server
            .grep(Parameters(GrepParams {
                path: dir.path().to_str().unwrap().to_string(),
                pattern: "hello".to_string(),
                case_insensitive: Some(true),
                dot_matches_newline: None,
                context_lines: None,
            }))
            .await
            .unwrap();
        assert!(result.contains("Hello World"));
    }

    #[tokio::test]
    async fn grep_dir_no_match_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "foo bar\n")
            .await
            .unwrap();

        let server = crate::mcp::make_test_server(dir.path()).await;
        let result = server
            .grep(Parameters(GrepParams {
                path: dir.path().to_str().unwrap().to_string(),
                pattern: "zzz".to_string(),
                case_insensitive: None,
                dot_matches_newline: None,
                context_lines: None,
            }))
            .await
            .unwrap();
        assert_eq!(result, "");
    }

    #[tokio::test]
    async fn grep_dir_context_lines() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "before\nmatch\nafter\n")
            .await
            .unwrap();

        let server = crate::mcp::make_test_server(dir.path()).await;
        let result = server
            .grep(Parameters(GrepParams {
                path: dir.path().to_str().unwrap().to_string(),
                pattern: "match".to_string(),
                case_insensitive: None,
                dot_matches_newline: None,
                context_lines: Some(1),
            }))
            .await
            .unwrap();
        assert!(result.contains("1-before"));
        assert!(result.contains("2:match"));
        assert!(result.contains("3-after"));
    }

    #[tokio::test]
    async fn grep_dir_invalid_regex_errors() {
        let dir = tempfile::tempdir().unwrap();

        let server = crate::mcp::make_test_server(dir.path()).await;
        let err = server
            .grep(Parameters(GrepParams {
                path: dir.path().to_str().unwrap().to_string(),
                pattern: "[invalid".to_string(),
                case_insensitive: None,
                dot_matches_newline: None,
                context_lines: None,
            }))
            .await
            .unwrap_err();
        assert!(err.message.contains("Invalid regex"));
    }

    #[tokio::test]
    async fn grep_on_file_works() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        fs::write(&file, "content\n").await.unwrap();

        let server = crate::mcp::make_test_server(dir.path()).await;
        let result = server
            .grep(Parameters(GrepParams {
                path: file.to_str().unwrap().to_string(),
                pattern: "content".to_string(),
                case_insensitive: None,
                dot_matches_newline: None,
                context_lines: None,
            }))
            .await
            .unwrap();
        assert!(result.contains("1:content"));
    }

    #[tokio::test]
    async fn grep_dir_outside_allowlist_errors() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();

        let server = crate::mcp::make_test_server(dir.path()).await;
        let err = server
            .grep(Parameters(GrepParams {
                path: other.path().to_str().unwrap().to_string(),
                pattern: "foo".to_string(),
                case_insensitive: None,
                dot_matches_newline: None,
                context_lines: None,
            }))
            .await
            .unwrap_err();
        assert!(err.message.contains("EPERM"));
    }
}
