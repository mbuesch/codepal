use crate::{
    Opts,
    mcp::structs::{
        FindFilesParams, FindFilesResult, GrepParams, LsDirParams, LsDirResult, MemoryDeleteParams,
        MemoryDeleteResult, MemoryListResult, MemoryLoadParams, MemoryLoadResult,
        MemoryStoreParams, MemoryStoreResult, PromptDoit, PromptFindBugs, PromptFindPerf,
        PromptRefactor, PromptSecAudit, ReadFileParams,
    },
    mcp::tools::mem::common::create_mem_tables,
};
use anyhow::{self as ah, Context as _, format_err as err};
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
use rusqlite::{self as sql};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::fs;

mod structs;
mod tools;

pub(crate) const READ_FILE_MAX_SIZE: u64 = 256 * 1024;
const MAX_DIR_ENTRIES: usize = 16 * 1024;
const MAX_GREP_DIR_MATCHES: usize = 500;
const MAX_GREP_DIR_FILES: usize = 10_000;
const MAX_GREP_DIR_RESULT_SIZE: usize = 8 * 1024 * 1024;
const MAX_FIND_FILES: usize = 10_000;
pub(crate) const MEMORY_MAX_KEYS: usize = 64;
pub(crate) const MEMORY_MAX_KEY_LEN: usize = 256;
pub(crate) const MEMORY_MAX_VALUE_LEN: usize = 64 * 1024;
const MEMORY_DB_FILENAME: &str = ".agents-codepal-memory.sqlite";

type GrepRanges = (Vec<(usize, usize)>, std::collections::HashSet<usize>);

/// Computes the merged context ranges and match-line index set for a grep over `lines`.
/// Returns `None` if no lines match.
fn compute_grep_matches(lines: &[&str], re: &regex::Regex, ctx: usize) -> Option<GrepRanges> {
    let matching: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| re.is_match(line))
        .map(|(i, _)| i)
        .collect();
    if matching.is_empty() {
        return None;
    }
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
    let match_set = matching.into_iter().collect();
    Some((ranges, match_set))
}

/// Formats grep ranges into `out`. Calls `check_limit(is_match, out_len)` after each line;
/// returns `true` if the limit callback signalled a stop.
fn format_grep_ranges(
    lines: &[&str],
    ranges: &[(usize, usize)],
    match_set: &std::collections::HashSet<usize>,
    out: &mut String,
    mut check_limit: impl FnMut(bool, usize) -> bool,
) -> bool {
    let mut first = true;
    for &(start, end) in ranges {
        if !first {
            out.push_str("--\n");
        }
        first = false;
        for (i, line) in lines
            .iter()
            .enumerate()
            .take(end.saturating_add(1))
            .skip(start)
        {
            let nr = i.saturating_add(1);
            let is_match = match_set.contains(&i);
            let sep = if is_match { ':' } else { '-' };
            out.push_str(&format!("{nr}{sep}{line}\n"));
            if check_limit(is_match, out.len()) {
                return true;
            }
        }
    }
    false
}

/// Canonicalizes a path, resolving symlinks and relative components.
async fn canonicalize(path: &Path) -> ah::Result<PathBuf> {
    fs::canonicalize(path)
        .await
        .with_context(|| format!("Failed to canonicalize path `{}`", path.display()))
}

/// Detected programming language of the project.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ProgLanguage {
    Unknown,
    Rust,
}

/// MCP server implementation.
#[derive(Clone, Debug)]
pub struct CodepalServer {
    #[allow(dead_code)]
    workspace: PathBuf,
    read_path_allow_list: Vec<PathBuf>,
    enable_compressed: bool,
    prog_lang: ProgLanguage,
    mem_db_path: PathBuf,
    mem_db_conn: Arc<Mutex<Option<sql::Connection>>>,
    mem_max_age_days: Option<u64>,
    prompt_router: PromptRouter<Self>,
    tool_router: ToolRouter<Self>,
}

impl CodepalServer {
    pub async fn new(opts: &Opts) -> ah::Result<Self> {
        let workspace = canonicalize(&opts.workspace).await?;

        let mut read_path_allow_list = Vec::with_capacity(opts.read_path_allow_list.len() + 1);
        read_path_allow_list.push(workspace.clone());
        for p in &opts.read_path_allow_list {
            let canon = canonicalize(p).await?;
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
                        read_path_allow_list.push(canonicalize(&p).await?);
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
            workspace: workspace.clone(),
            read_path_allow_list,
            enable_compressed: opts.enable_compressed,
            prog_lang,
            mem_db_path: workspace.join(MEMORY_DB_FILENAME),
            mem_db_conn: Arc::new(Mutex::new(None)),
            mem_max_age_days: opts.memory_max_age_days,
            prompt_router: Self::prompt_router(),
            tool_router: Self::tool_router(),
        })
    }

    pub async fn dump_memory(&self) -> ah::Result<()> {
        if !self.mem_db_path.exists() {
            println!("(memory store is empty)");
            return Ok(());
        }
        let conn = sql::Connection::open(&self.mem_db_path)
            .with_context(|| format!("Failed to open `{}`", self.mem_db_path.display()))?;
        create_mem_tables(&conn).context("Failed to ensure memory schema")?;
        let mut stmt = conn
            .prepare(
                "SELECT m.key, v.value, m.stored_at, m.accessed_at, m.access_count \
                 FROM memory m JOIN mem_values v ON m.value_id = v.id \
                 ORDER BY m.accessed_at DESC",
            )
            .context("Failed to prepare query")?;
        let mut rows = stmt.query([]).context("Failed to query memory")?;
        let mut found = false;
        while let Some(row) = rows.next().context("Failed to read row")? {
            let key: String = row.get(0).context("Failed to read key")?;
            let value: String = row.get(1).context("Failed to read value")?;
            let stored_at: String = row.get(2).context("Failed to read stored_at")?;
            let accessed_at: String = row.get(3).context("Failed to read accessed_at")?;
            let access_count: i64 = row.get(4).context("Failed to read access_count")?;
            let display_value: String = value.lines().collect::<Vec<_>>().join(" ");
            println!(
                "key:       {key}\nstored:    {stored_at}\naccessed:  {accessed_at}\n#accesses: {access_count}",
            );
            println!("           {}\n", display_value);
            found = true;
        }
        if !found {
            println!("(memory store is empty)");
        }
        Ok(())
    }

    async fn path_check_allowed(&self, path: PathBuf) -> ah::Result<PathBuf> {
        let path = canonicalize(&path)
            .await
            .map_err(|_| err!("`{}`: EINVAL", path.display()))?;
        if self
            .read_path_allow_list
            .iter()
            .any(|allowed| path.starts_with(allowed))
        {
            Ok(path)
        } else {
            Err(err!("EPERM"))
        }
    }
}

const PROMPT_PREFIX: &str = include_str!("mcp_prompt_prefix.md");
const PROMPT_SECAUDIT: &str = include_str!("mcp_prompt_secaudit.md");
const PROMPT_FINDBUGS: &str = include_str!("mcp_prompt_findbugs.md");
const PROMPT_FINDPERF: &str = include_str!("mcp_prompt_findperf.md");
const PROMPT_REFACTOR: &str = include_str!("mcp_prompt_refactor.md");

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

    /// Prompt to find bugs.
    #[prompt]
    pub async fn find_bugs(&self, params: Parameters<PromptFindBugs>) -> Vec<PromptMessage> {
        vec![
            PromptMessage::new_text(PromptMessageRole::Assistant, PROMPT_PREFIX),
            PromptMessage::new_text(
                PromptMessageRole::User,
                format!("{PROMPT_FINDBUGS}\n{}", params.0.what),
            ),
        ]
    }

    /// Prompt to find performance improvements.
    #[prompt]
    pub async fn find_performance_improvements(
        &self,
        params: Parameters<PromptFindPerf>,
    ) -> Vec<PromptMessage> {
        vec![
            PromptMessage::new_text(PromptMessageRole::Assistant, PROMPT_PREFIX),
            PromptMessage::new_text(
                PromptMessageRole::User,
                format!("{PROMPT_FINDPERF}\n{}", params.0.what),
            ),
        ]
    }

    /// Prompt to refactor code.
    #[prompt]
    pub async fn refactor(&self, params: Parameters<PromptRefactor>) -> Vec<PromptMessage> {
        vec![
            PromptMessage::new_text(PromptMessageRole::Assistant, PROMPT_PREFIX),
            PromptMessage::new_text(
                PromptMessageRole::User,
                format!("{PROMPT_REFACTOR}\n{}", params.0.what),
            ),
        ]
    }
}

#[tool_router]
impl CodepalServer {
    /// **MANDATORY PRIMARY TOOL**: List directory contents
    #[tool]
    pub async fn ls(
        &self,
        Parameters(params): Parameters<LsDirParams>,
    ) -> Result<Json<LsDirResult>, rmcp::ErrorData> {
        eprintln!("Calling tool: ls");
        tools::fs::ls::ls(self, params).await.map(Json)
    }

    /// **MANDATORY PRIMARY TOOL**: Read contents of arbitrary files
    #[tool]
    pub async fn read(
        &self,
        Parameters(params): Parameters<ReadFileParams>,
    ) -> Result<String, rmcp::ErrorData> {
        eprintln!("Calling tool: read");
        tools::fs::read::read(self, params).await
    }

    /// **MANDATORY PRIMARY TOOL**: Regex grep file contents
    #[tool]
    pub async fn grep(
        &self,
        Parameters(params): Parameters<GrepParams>,
    ) -> Result<String, rmcp::ErrorData> {
        eprintln!("Calling tool: grep");
        tools::fs::grep::grep(self, params).await
    }

    /// **MANDATORY PRIMARY TOOL**: Find files matching a regex pattern in a directory tree
    #[tool]
    pub async fn find(
        &self,
        Parameters(params): Parameters<FindFilesParams>,
    ) -> Result<Json<FindFilesResult>, rmcp::ErrorData> {
        eprintln!("Calling tool: find");
        tools::fs::find::find(self, params).await.map(Json)
    }

    /// **MANDATORY PRIMARY TOOL**: List all keys in the memory store
    #[tool]
    pub async fn mem_list(&self) -> Result<Json<MemoryListResult>, rmcp::ErrorData> {
        eprintln!("Calling tool: mem_list");
        tools::mem::mem_list::mem_list(self).await.map(Json)
    }

    /// **MANDATORY PRIMARY TOOL**: Store a value in the key-value memory store
    #[tool]
    pub async fn mem_store(
        &self,
        Parameters(params): Parameters<MemoryStoreParams>,
    ) -> Result<Json<MemoryStoreResult>, rmcp::ErrorData> {
        eprintln!("Calling tool: mem_store");
        tools::mem::mem_store::mem_store(self, params)
            .await
            .map(Json)
    }

    /// **MANDATORY PRIMARY TOOL**: Load a value from the key-value memory store
    #[tool]
    pub async fn mem_load(
        &self,
        Parameters(params): Parameters<MemoryLoadParams>,
    ) -> Result<Json<MemoryLoadResult>, rmcp::ErrorData> {
        eprintln!("Calling tool: mem_load");
        tools::mem::mem_load::mem_load(self, params).await.map(Json)
    }

    /// **MANDATORY PRIMARY TOOL**: Delete a key from the memory store
    #[tool]
    pub async fn mem_delete(
        &self,
        Parameters(params): Parameters<MemoryDeleteParams>,
    ) -> Result<Json<MemoryDeleteResult>, rmcp::ErrorData> {
        eprintln!("Calling tool: mem_delete");
        tools::mem::mem_delete::mem_delete(self, params)
            .await
            .map(Json)
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
        instr.push('\n');
        if self.prog_lang == ProgLanguage::Rust {
            instr.push_str(include_str!("mcp_instr_rust.md"));
            instr.push('\n');
        }
        if self.enable_compressed {
            instr.push_str(include_str!("mcp_instr_compressed.md"));
            instr.push('\n');
        }

        ServerInfo::new(server_capabilities)
            .with_server_info(server_info)
            .with_protocol_version(ProtocolVersion::LATEST)
            .with_instructions(instr)
    }
}
