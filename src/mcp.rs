use crate::{
    Opts,
    mcp_struct::{
        GrepFileParams, LsDirParams, LsDirResult, PromptDoit, PromptSecAudit, ReadFileParams,
    },
};
use anyhow::{self as ah, format_err as err};
use rmcp::{
    RoleServer, ServerHandler,
    handler::server::{
        router::{prompt::PromptRouter, tool::ToolRouter},
        wrapper::{Json, Parameters},
    },
    model::{
        GetPromptRequestParams, GetPromptResult, Implementation, ListPromptsResult,
        PaginatedRequestParams, PromptMessage, PromptMessageRole, ProtocolVersion,
        ServerCapabilities, ServerInfo,
    },
    prompt, prompt_handler, prompt_router,
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use std::path::PathBuf;
use tokio::fs;

const READ_FILE_MAX_SIZE: u64 = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ProgLanguage {
    Unknown,
    Rust,
}

#[derive(Clone, Debug)]
pub struct CodepalServer {
    #[allow(dead_code)]
    workspace: PathBuf,
    read_path_allow_list: Vec<PathBuf>,
    enable_compressed: bool,
    #[allow(dead_code)]
    prog_lang: ProgLanguage,
    prompt_router: PromptRouter<Self>,
    tool_router: ToolRouter<Self>,
}

impl CodepalServer {
    pub async fn new(opts: &Opts) -> ah::Result<Self> {
        let workspace = fs::canonicalize(&opts.workspace).await.map_err(|e| {
            err!(
                "Failed to canonicalize workspace path `{}`: {e}",
                opts.workspace.display()
            )
        })?;

        let mut read_path_allow_list = Vec::with_capacity(opts.read_path_allow_list.len() + 1);
        read_path_allow_list.push(workspace.clone());
        for p in &opts.read_path_allow_list {
            let canon = fs::canonicalize(&p)
                .await
                .map_err(|e| err!("Failed to canonicalize path `{}`: {e}", p.display()))?;
            read_path_allow_list.push(canon);
        }

        // Project programming language detection: Rust
        let mut prog_lang = ProgLanguage::Unknown;
        if workspace.join("Cargo.toml").exists() {
            eprintln!("Detected Rust project.");
            prog_lang = ProgLanguage::Rust;
            if !opts.no_auto_path_allow
                && let Ok(home) = std::env::var("HOME")
            {
                let home = PathBuf::from(home);
                for dir in [".cargo", ".rustup"] {
                    let p = home.join(dir);
                    if p.is_dir() {
                        eprintln!("Auto-allowing Rust read-path: {}", p.display());
                        read_path_allow_list.push(p);
                    }
                }
            }
        }

        if !read_path_allow_list.is_empty() {
            eprintln!("File read path allowlist:");
            for p in &read_path_allow_list {
                eprintln!(" - {}", p.display());
            }
        }

        Ok(Self {
            workspace,
            read_path_allow_list,
            enable_compressed: opts.enable_compressed,
            prog_lang,
            prompt_router: Self::prompt_router(),
            tool_router: Self::tool_router(),
        })
    }

    async fn path_check_allowed(&self, path: PathBuf) -> ah::Result<PathBuf> {
        let path = fs::canonicalize(&path).await.map_err(|_| {
            rmcp::ErrorData::invalid_params(format!("`{}`: EINVAL", path.display()), None)
        })?;
        if self
            .read_path_allow_list
            .iter()
            .any(|allowed| path.starts_with(allowed))
        {
            Ok(path)
        } else {
            Err(err!("`{}`: EPERM", path.display()))
        }
    }
}

const PROMPT_PREFIX: &str = include_str!("mcp_prompt_prefix.md");
const PROMPT_SECAUDIT: &str = include_str!("mcp_prompt_secaudit.md");

#[prompt_router]
impl CodepalServer {
    /// Prompt with CodePal instructions.
    #[prompt]
    pub async fn doit(&self, params: Parameters<PromptDoit>) -> Vec<PromptMessage> {
        vec![
            PromptMessage::new_text(PromptMessageRole::Assistant, PROMPT_PREFIX),
            PromptMessage::new_text(PromptMessageRole::User, params.0.instructions),
        ]
    }

    /// Prompt to perform a security audit.
    #[prompt]
    pub async fn security_audit(&self, params: Parameters<PromptSecAudit>) -> Vec<PromptMessage> {
        vec![
            PromptMessage::new_text(PromptMessageRole::Assistant, PROMPT_PREFIX),
            PromptMessage::new_text(
                PromptMessageRole::User,
                format!("{PROMPT_SECAUDIT}\n{}", params.0.what),
            ),
        ]
    }
}

#[tool_router]
impl CodepalServer {
    /// List directory contents
    #[tool]
    pub async fn ls_dir(
        &self,
        Parameters(LsDirParams { path }): Parameters<LsDirParams>,
    ) -> Result<Json<LsDirResult>, rmcp::ErrorData> {
        let path = self
            .path_check_allowed(path.into())
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

        let mut entries = vec![];
        while let Some(entry) = read_dir.next_entry().await.map_err(|_| {
            rmcp::ErrorData::invalid_params(format!("`{}`: EPERM", path.display()), None)
        })? {
            if let Ok(file_type) = entry.file_type().await {
                let suffix = if file_type.is_dir() { "/" } else { "" };
                entries.push(format!("{}{suffix}", entry.file_name().to_string_lossy()));
            }
        }
        entries.sort();

        Ok(Json(LsDirResult { entries }))
    }

    /// Read contents of arbitrary files
    #[tool]
    pub async fn read_file(
        &self,
        Parameters(ReadFileParams {
            path,
            start_line,
            end_line,
        }): Parameters<ReadFileParams>,
    ) -> Result<String, rmcp::ErrorData> {
        let path = self
            .path_check_allowed(path.into())
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

        let start: usize = if let Some(start_line) = start_line {
            start_line
                .saturating_sub(1)
                .try_into()
                .map_err(|_| rmcp::ErrorData::invalid_params("Invalid line number start", None))?
        } else {
            0
        };
        let end: usize = if let Some(end_line) = end_line {
            let end: usize = end_line
                .try_into()
                .map_err(|_| rmcp::ErrorData::invalid_params("Invalid line number end", None))?;
            end.min(lines.len())
        } else {
            lines.len()
        };

        Ok(lines[start..end].join("\n"))
    }

    /// Regex grep file contents
    #[tool]
    pub async fn grep_file(
        &self,
        Parameters(GrepFileParams {
            path,
            pattern,
            case_insensitive,
            dot_matches_newline,
            context_lines,
        }): Parameters<GrepFileParams>,
    ) -> Result<String, rmcp::ErrorData> {
        let path = self
            .path_check_allowed(path.into())
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

        let re = regex::RegexBuilder::new(&pattern)
            .case_insensitive(case_insensitive.unwrap_or(false))
            .dot_matches_new_line(dot_matches_newline.unwrap_or(false))
            .build()
            .map_err(|e| rmcp::ErrorData::invalid_params(format!("Invalid regex: {e}"), None))?;

        let content = fs::read_to_string(&path).await.map_err(|e| {
            rmcp::ErrorData::invalid_params(format!("`{}`: Read error: {e}", path.display()), None)
        })?;

        let lines: Vec<&str> = content.lines().collect();
        let ctx = context_lines.unwrap_or(0) as usize;

        let matching: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| re.is_match(line))
            .map(|(i, _)| i)
            .collect();

        if matching.is_empty() {
            return Ok(String::new());
        }

        // Merge overlapping context windows into ranges.
        let mut ranges: Vec<(usize, usize)> = vec![];
        for &m in &matching {
            let start = m.saturating_sub(ctx);
            let end = (m + ctx).min(lines.len().saturating_sub(1));
            if let Some(last) = ranges.last_mut()
                && start <= last.1 + 1
            {
                last.1 = last.1.max(end);
                continue;
            }
            ranges.push((start, end));
        }

        let match_set: std::collections::HashSet<usize> = matching.into_iter().collect();

        let mut result = String::new();
        let mut first = true;
        for (start, end) in ranges {
            if !first {
                result.push_str("--\n");
            }
            first = false;
            for (i, line) in lines
                .iter()
                .enumerate()
                .take(end.saturating_add(1))
                .skip(start)
            {
                let nr = i.saturating_add(1);
                let sep = if match_set.contains(&i) { ':' } else { '-' };
                result.push_str(&format!("{nr}{sep}{line}\n"));
            }
        }

        Ok(result)
    }
}

#[prompt_handler(router = self.prompt_router)]
#[tool_handler(router = self.tool_router)]
impl ServerHandler for CodepalServer {
    fn get_info(&self) -> ServerInfo {
        let server_capabilities = ServerCapabilities::builder()
            .enable_prompts()
            .enable_tools()
            .build();

        let server_info = Implementation::new("CodePal", env!("CARGO_PKG_VERSION"));

        let mut instr = String::new();
        instr.push_str(include_str!("mcp_instr.md"));
        if self.enable_compressed {
            instr.push_str(include_str!("mcp_instr_compressed.md"));
        }

        ServerInfo::new(server_capabilities)
            .with_server_info(server_info)
            .with_protocol_version(ProtocolVersion::LATEST)
            .with_instructions(instr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    async fn make_server(workspace: &Path) -> CodepalServer {
        let opts = Opts {
            workspace: workspace.to_path_buf(),
            read_path_allow_list: vec![],
            no_auto_path_allow: false,
            enable_compressed: false,
        };
        CodepalServer::new(&opts)
            .await
            .expect("server creation failed")
    }

    #[tokio::test]
    async fn ls_dir_returns_sorted_entries() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("b.txt"), "").await.unwrap();
        fs::write(dir.path().join("a.txt"), "").await.unwrap();
        fs::create_dir(dir.path().join("subdir")).await.unwrap();

        let server = make_server(dir.path()).await;
        let result = server
            .ls_dir(Parameters(LsDirParams {
                path: dir.path().to_str().unwrap().to_string(),
            }))
            .await
            .unwrap();
        assert_eq!(result.0.entries, vec!["a.txt", "b.txt", "subdir/"]);
    }

    #[tokio::test]
    async fn ls_dir_on_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        fs::write(&file, "hi").await.unwrap();

        let server = make_server(dir.path()).await;
        let err = server
            .ls_dir(Parameters(LsDirParams {
                path: file.to_str().unwrap().to_string(),
            }))
            .await
            .err()
            .expect("expected Err");
        assert!(err.message.contains("Not a directory"));
    }

    #[tokio::test]
    async fn ls_dir_outside_allowlist_errors() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();

        let server = make_server(dir.path()).await;
        let err = server
            .ls_dir(Parameters(LsDirParams {
                path: other.path().to_str().unwrap().to_string(),
            }))
            .await
            .err()
            .expect("expected Err");
        assert!(err.message.contains("EPERM"));
    }

    #[tokio::test]
    async fn read_file_entire_content() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        fs::write(&file, "line1\nline2\nline3\n").await.unwrap();

        let server = make_server(dir.path()).await;
        let content = server
            .read_file(Parameters(ReadFileParams {
                path: file.to_str().unwrap().to_string(),
                start_line: None,
                end_line: None,
            }))
            .await
            .unwrap();
        assert_eq!(content, "line1\nline2\nline3");
    }

    #[tokio::test]
    async fn read_file_with_start_line() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        fs::write(&file, "line1\nline2\nline3\n").await.unwrap();

        let server = make_server(dir.path()).await;
        let content = server
            .read_file(Parameters(ReadFileParams {
                path: file.to_str().unwrap().to_string(),
                start_line: Some(2),
                end_line: None,
            }))
            .await
            .unwrap();
        assert_eq!(content, "line2\nline3");
    }

    #[tokio::test]
    async fn read_file_with_end_line() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        fs::write(&file, "line1\nline2\nline3\n").await.unwrap();

        let server = make_server(dir.path()).await;
        let content = server
            .read_file(Parameters(ReadFileParams {
                path: file.to_str().unwrap().to_string(),
                start_line: None,
                end_line: Some(2),
            }))
            .await
            .unwrap();
        assert_eq!(content, "line1\nline2");
    }

    #[tokio::test]
    async fn read_file_with_line_range() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        fs::write(&file, "line1\nline2\nline3\nline4\n")
            .await
            .unwrap();

        let server = make_server(dir.path()).await;
        let content = server
            .read_file(Parameters(ReadFileParams {
                path: file.to_str().unwrap().to_string(),
                start_line: Some(2),
                end_line: Some(3),
            }))
            .await
            .unwrap();
        assert_eq!(content, "line2\nline3");
    }

    #[tokio::test]
    async fn read_file_on_directory_errors() {
        let dir = tempfile::tempdir().unwrap();

        let server = make_server(dir.path()).await;
        let err = server
            .read_file(Parameters(ReadFileParams {
                path: dir.path().to_str().unwrap().to_string(),
                start_line: None,
                end_line: None,
            }))
            .await
            .unwrap_err();
        assert!(err.message.contains("Not a file"));
    }

    #[tokio::test]
    async fn read_file_too_large_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("big.bin");
        fs::write(&file, vec![b'x'; (READ_FILE_MAX_SIZE + 1) as usize])
            .await
            .unwrap();

        let server = make_server(dir.path()).await;
        let err = server
            .read_file(Parameters(ReadFileParams {
                path: file.to_str().unwrap().to_string(),
                start_line: None,
                end_line: None,
            }))
            .await
            .unwrap_err();
        assert!(err.message.contains("File too large"));
    }

    #[tokio::test]
    async fn read_file_outside_allowlist_errors() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let file = other.path().join("secret.txt");
        fs::write(&file, "secret").await.unwrap();

        let server = make_server(dir.path()).await;
        let err = server
            .read_file(Parameters(ReadFileParams {
                path: file.to_str().unwrap().to_string(),
                start_line: None,
                end_line: None,
            }))
            .await
            .unwrap_err();
        assert!(err.message.contains("EPERM"));
    }

    #[tokio::test]
    async fn grep_file_basic_match() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("code.txt");
        fs::write(&file, "foo bar\nbaz\nfoo qux\n").await.unwrap();

        let server = make_server(dir.path()).await;
        let result = server
            .grep_file(Parameters(GrepFileParams {
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

        let server = make_server(dir.path()).await;
        let result = server
            .grep_file(Parameters(GrepFileParams {
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

        let server = make_server(dir.path()).await;
        let result = server
            .grep_file(Parameters(GrepFileParams {
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

        let server = make_server(dir.path()).await;
        let result = server
            .grep_file(Parameters(GrepFileParams {
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

        let server = make_server(dir.path()).await;
        let err = server
            .grep_file(Parameters(GrepFileParams {
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

        let server = make_server(dir.path()).await;
        let err = server
            .grep_file(Parameters(GrepFileParams {
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
}
