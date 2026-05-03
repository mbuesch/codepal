use crate::{
    Opts,
    mcp::structs::{
        FindFilesParams, FindFilesResult, GrepParams, LsDirParams, LsDirResult, MemoryDeleteParams,
        MemoryDeleteResult, MemoryListResult, MemoryLoadParams, MemoryLoadResult,
        MemoryStoreParams, MemoryStoreResult, PromptDoit, PromptFindBugs, PromptFindPerf,
        PromptRefactor, PromptSecAudit, ReadFileParams,
    },
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
use rusqlite::{self as sql, OptionalExtension as _};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::fs;
use walkdir::WalkDir;

mod structs;
#[cfg(test)]
mod tests;

const READ_FILE_MAX_SIZE: u64 = 256 * 1024;
const MAX_DIR_ENTRIES: usize = 16 * 1024;
const MAX_GREP_DIR_MATCHES: usize = 500;
const MAX_GREP_DIR_FILES: usize = 10_000;
const MAX_GREP_DIR_RESULT_SIZE: usize = 8 * 1024 * 1024;
const MAX_FIND_FILES: usize = 10_000;
const MEMORY_MAX_KEYS: usize = 64;
const MEMORY_MAX_KEY_LEN: usize = 256;
const MEMORY_MAX_VALUE_LEN: usize = 64 * 1024;
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

fn create_mem_tables(conn: &sql::Connection) -> sql::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mem_values (
            id INTEGER PRIMARY KEY,
            value TEXT NOT NULL UNIQUE
        );
        CREATE TABLE IF NOT EXISTS memory (
            key TEXT PRIMARY KEY,
            value_id INTEGER NOT NULL REFERENCES mem_values(id),
            stored_at TEXT NOT NULL DEFAULT (datetime('now')),
            accessed_at TEXT NOT NULL DEFAULT (datetime('now')),
            access_count INTEGER NOT NULL DEFAULT 0
        );",
    )
}

fn prune_expired_entries(conn: &sql::Connection, max_age_days: u64) -> sql::Result<()> {
    let modifier = format!("-{max_age_days} days");
    let n = conn.execute(
        "DELETE FROM memory WHERE accessed_at < datetime('now', ?1)",
        sql::params![modifier],
    )?;
    if n > 0 {
        eprintln!(
            "memory: pruned {n} expired entr{}.",
            if n == 1 { "y" } else { "ies" }
        );
        conn.execute(
            "DELETE FROM mem_values WHERE id NOT IN (SELECT value_id FROM memory)",
            [],
        )?;
    }
    Ok(())
}

async fn canonicalize(path: &Path) -> ah::Result<PathBuf> {
    fs::canonicalize(path)
        .await
        .with_context(|| format!("Failed to canonicalize path `{}`", path.display()))
}

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
            if !found {
                println!(
                    "{:<40}  {:<19}  {:<19}  {:>7}  VALUE",
                    "KEY", "STORED", "ACCESSED", "ACCESSES"
                );
                println!("{}", "-".repeat(100));
            }
            let key: String = row.get(0).context("Failed to read key")?;
            let value: String = row.get(1).context("Failed to read value")?;
            let stored_at: String = row.get(2).context("Failed to read stored_at")?;
            let accessed_at: String = row.get(3).context("Failed to read accessed_at")?;
            let access_count: i64 = row.get(4).context("Failed to read access_count")?;
            // Truncate value for display — replace newlines with spaces.
            let display_value: String = value
                .lines()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(80)
                .collect();
            let display_value = if value.len() > display_value.len() {
                format!("{display_value}…")
            } else {
                display_value
            };
            println!(
                "{:<40}  {:<19}  {:<19}  {:>7}  {}",
                key, stored_at, accessed_at, access_count, display_value
            );
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
    /// List directory contents
    #[tool]
    pub async fn ls(
        &self,
        Parameters(LsDirParams { path }): Parameters<LsDirParams>,
    ) -> Result<Json<LsDirResult>, rmcp::ErrorData> {
        eprintln!("Calling tool: ls");
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

        Ok(Json(LsDirResult { entries }))
    }

    /// Read contents of arbitrary files
    #[tool]
    pub async fn read(
        &self,
        Parameters(ReadFileParams {
            path,
            start_line,
            end_line,
        }): Parameters<ReadFileParams>,
    ) -> Result<String, rmcp::ErrorData> {
        eprintln!("Calling tool: read");
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

    /// Regex grep file contents
    #[tool]
    pub async fn grep(
        &self,
        Parameters(GrepParams {
            path,
            pattern,
            case_insensitive,
            dot_matches_newline,
            context_lines,
        }): Parameters<GrepParams>,
    ) -> Result<String, rmcp::ErrorData> {
        eprintln!("Calling tool: grep");
        let path = self
            .path_check_allowed(path.into())
            .await
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        let meta = fs::metadata(&path).await.map_err(|_| {
            rmcp::ErrorData::invalid_params(format!("`{}`: EPERM", path.display()), None)
        })?;

        let re = regex::RegexBuilder::new(&pattern)
            .case_insensitive(case_insensitive.unwrap_or(false))
            .dot_matches_new_line(dot_matches_newline.unwrap_or(false))
            .build()
            .map_err(|e| rmcp::ErrorData::invalid_params(format!("Invalid regex: {e}"), None))?;

        let ctx = context_lines.unwrap_or(0) as usize;

        if meta.is_file() {
            if meta.len() > READ_FILE_MAX_SIZE {
                return Err(rmcp::ErrorData::invalid_params(
                    format!("`{}`: File too large", path.display()),
                    None,
                ));
            }

            let content = fs::read_to_string(&path).await.map_err(|e| {
                rmcp::ErrorData::invalid_params(
                    format!("`{}`: Read error: {e}", path.display()),
                    None,
                )
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

    /// Find files matching a regex pattern in a directory tree
    #[tool]
    pub async fn find(
        &self,
        Parameters(FindFilesParams {
            path,
            pattern,
            case_insensitive,
        }): Parameters<FindFilesParams>,
    ) -> Result<Json<FindFilesResult>, rmcp::ErrorData> {
        eprintln!("Calling tool: find");
        let dir = self
            .path_check_allowed(path.into())
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

        let re = regex::RegexBuilder::new(&pattern)
            .case_insensitive(case_insensitive.unwrap_or(true))
            .build()
            .map_err(|e| rmcp::ErrorData::invalid_params(format!("Invalid regex: {e}"), None))?;

        let dir_clone = dir.clone();
        let files: Vec<String> = tokio::task::spawn_blocking(move || {
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
                .take(MAX_FIND_FILES)
                .collect()
        })
        .await
        .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;

        Ok(Json(FindFilesResult { files }))
    }

    /// List all keys in the memory store
    #[tool]
    pub async fn mem_list(&self) -> Result<Json<MemoryListResult>, rmcp::ErrorData> {
        eprintln!("Calling tool: mem_list");
        let mut guard = self.mem_db_conn.lock().expect("Lock poisoned");
        if guard.is_none() {
            if !self.mem_db_path.exists() {
                return Ok(Json(MemoryListResult { keys: vec![] }));
            }
            let conn = sql::Connection::open(&self.mem_db_path)
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
            create_mem_tables(&conn)
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
            *guard = Some(conn);
        }
        let conn = guard.as_mut().unwrap();
        let mut stmt = conn
            .prepare("SELECT key FROM memory ORDER BY accessed_at DESC")
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        let keys: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(Json(MemoryListResult { keys }))
    }

    /// Store a value in the key-value memory store
    #[tool]
    pub async fn mem_store(
        &self,
        Parameters(MemoryStoreParams { keys, value }): Parameters<MemoryStoreParams>,
    ) -> Result<Json<MemoryStoreResult>, rmcp::ErrorData> {
        eprintln!("Calling tool: mem_store");
        if keys.is_empty() {
            return Err(rmcp::ErrorData::invalid_params(
                "keys must not be empty",
                None,
            ));
        }
        if keys.len() > MEMORY_MAX_KEYS {
            return Err(rmcp::ErrorData::invalid_params(
                format!("too many keys (max {MEMORY_MAX_KEYS})"),
                None,
            ));
        }
        for key in &keys {
            if key.len() > MEMORY_MAX_KEY_LEN {
                return Err(rmcp::ErrorData::invalid_params(
                    format!("key too long (max {MEMORY_MAX_KEY_LEN} bytes)"),
                    None,
                ));
            }
        }
        if value.len() > MEMORY_MAX_VALUE_LEN {
            return Err(rmcp::ErrorData::invalid_params(
                format!("value too long (max {} bytes)", MEMORY_MAX_VALUE_LEN),
                None,
            ));
        }
        let mut guard = self.mem_db_conn.lock().expect("Lock poisoned");
        if guard.is_none() {
            let conn = sql::Connection::open(&self.mem_db_path)
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
            create_mem_tables(&conn)
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
            *guard = Some(conn);
        }
        let conn = guard.as_mut().unwrap();
        if let Some(max_age_days) = self.mem_max_age_days {
            prune_expired_entries(conn, max_age_days)
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        }
        // Insert the value once; get its id.
        conn.execute(
            "INSERT OR IGNORE INTO mem_values (value) VALUES (?1)",
            sql::params![value],
        )
        .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        let value_id: i64 = conn
            .query_row(
                "SELECT id FROM mem_values WHERE value = ?1",
                sql::params![value],
                |row| row.get(0),
            )
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        // Map every key to that value id, preserving access_count on overwrite.
        for key in &keys {
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
        Ok(Json(MemoryStoreResult { success: true }))
    }

    /// Load a value from the key-value memory store
    #[tool]
    pub async fn mem_load(
        &self,
        Parameters(MemoryLoadParams { keys }): Parameters<MemoryLoadParams>,
    ) -> Result<Json<MemoryLoadResult>, rmcp::ErrorData> {
        eprintln!("Calling tool: mem_load");
        let mut guard = self.mem_db_conn.lock().expect("Lock poisoned");
        if guard.is_none() {
            if !self.mem_db_path.exists() {
                return Ok(Json(MemoryLoadResult { value: None }));
            }
            let conn = sql::Connection::open(&self.mem_db_path)
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
            create_mem_tables(&conn)
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
            *guard = Some(conn);
        }
        let conn = guard.as_mut().unwrap();
        if let Some(max_age_days) = self.mem_max_age_days {
            prune_expired_entries(conn, max_age_days)
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        }
        let mut value = None;
        for key in &keys {
            let escaped = key
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            let found_key: Option<String> = conn
                .query_row(
                    "SELECT m.key FROM memory m WHERE m.key LIKE '%' || ?1 || '%' ESCAPE '\\' ORDER BY m.accessed_at DESC LIMIT 1",
                    sql::params![escaped],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
            if let Some(ref fk) = found_key {
                conn.execute(
                    "UPDATE memory SET accessed_at = datetime('now'), access_count = access_count + 1 WHERE key = ?1",
                    sql::params![fk],
                )
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
                value = conn
                    .query_row(
                        "SELECT v.value FROM memory m JOIN mem_values v ON m.value_id = v.id WHERE m.key = ?1",
                        sql::params![fk],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
                break;
            }
        }
        Ok(Json(MemoryLoadResult { value }))
    }

    /// Delete a key from the memory store
    #[tool]
    pub async fn mem_delete(
        &self,
        Parameters(MemoryDeleteParams { key }): Parameters<MemoryDeleteParams>,
    ) -> Result<Json<MemoryDeleteResult>, rmcp::ErrorData> {
        eprintln!("Calling tool: mem_delete");
        let mut guard = self.mem_db_conn.lock().expect("Lock poisoned");
        if guard.is_none() {
            if !self.mem_db_path.exists() {
                return Ok(Json(MemoryDeleteResult { found: false }));
            }
            let conn = sql::Connection::open(&self.mem_db_path)
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
            create_mem_tables(&conn)
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
            *guard = Some(conn);
        }
        let conn = guard.as_mut().unwrap();
        let n = conn
            .execute("DELETE FROM memory WHERE key = ?1", sql::params![key])
            .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        conn.execute(
            "DELETE FROM mem_values WHERE id NOT IN (SELECT value_id FROM memory)",
            [],
        )
        .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
        Ok(Json(MemoryDeleteResult { found: n > 0 }))
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
