use crate::{
    Opts,
    mcp::tools::{
        fs::{
            common::canonicalize,
            find::{FindFilesParams, FindFilesResult},
            grep::GrepParams,
            ls::{LsDirParams, LsDirResult},
            read::ReadFileParams,
        },
        mem::{
            common::{MEMORY_DB_FILENAME, create_mem_tables},
            mem_delete::{MemoryDeleteParams, MemoryDeleteResult},
            mem_list::MemoryListResult,
            mem_load::{MemoryLoadParams, MemoryLoadResult},
            mem_store::{MemoryStoreParams, MemoryStoreResult},
        },
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
use rusqlite::{self as sql};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

mod tools;

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
    enable_memory: bool,
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

        let mut tool_router = Self::tool_router();
        if !opts.enable_memory {
            for name in ["mem_list", "mem_store", "mem_load", "mem_delete"] {
                tool_router.map.remove(name);
            }
        }

        Ok(Self {
            workspace: workspace.clone(),
            read_path_allow_list,
            enable_compressed: opts.enable_compressed,
            enable_memory: opts.enable_memory,
            prog_lang,
            mem_db_path: workspace.join(MEMORY_DB_FILENAME),
            mem_db_conn: Arc::new(Mutex::new(None)),
            mem_max_age_days: opts.memory_max_age_days,
            prompt_router: Self::prompt_router(),
            tool_router,
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

    /// Opens or creates the memory DB, then calls `f` with the connection.
    pub(crate) fn with_mem_conn<T>(
        &self,
        f: impl FnOnce(&mut sql::Connection) -> Result<T, rmcp::ErrorData>,
    ) -> Result<T, rmcp::ErrorData> {
        let mut guard = self.mem_db_conn.lock().expect("Lock poisoned");
        if guard.is_none() {
            let conn = sql::Connection::open(&self.mem_db_path)
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
            create_mem_tables(&conn)
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
            *guard = Some(conn);
        }
        f(guard.as_mut().unwrap())
    }

    /// Opens the memory DB if the file exists, then calls `f` with the connection.
    /// Returns `default` without calling `f` when the DB file is not yet present.
    pub(crate) fn with_mem_conn_if_exists<T>(
        &self,
        default: T,
        f: impl FnOnce(&mut sql::Connection) -> Result<T, rmcp::ErrorData>,
    ) -> Result<T, rmcp::ErrorData> {
        let mut guard = self.mem_db_conn.lock().expect("Lock poisoned");
        if guard.is_none() {
            if !self.mem_db_path.exists() {
                return Ok(default);
            }
            let conn = sql::Connection::open(&self.mem_db_path)
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
            create_mem_tables(&conn)
                .map_err(|e| rmcp::ErrorData::invalid_params(e.to_string(), None))?;
            *guard = Some(conn);
        }
        f(guard.as_mut().unwrap())
    }

    pub(crate) fn mem_max_age_days(&self) -> Option<u64> {
        self.mem_max_age_days
    }

    fn resolve_prompt_vars(&self, text: &str, vars: &[(&str, &str)]) -> String {
        let mut text = text.to_string();

        if text.contains("$(ALLOWED_PATHS_LIST)") {
            let allowed_paths_list = self
                .read_path_allow_list
                .iter()
                .map(|p| format!("- {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n");
            text = text.replace("$(ALLOWED_PATHS_LIST)", &allowed_paths_list);
        }

        if text.contains("$(ENABLE_COMPRESSION)") {
            text = text.replace(
                "$(ENABLE_COMPRESSION)",
                if self.enable_compressed {
                    " **MUST ALWAYS** use **ultra-compressed** communication."
                } else {
                    ""
                },
            );
        }

        for (var, value) in vars {
            let placeholder = format!("$({var})");
            if text.contains(&placeholder) {
                text = text.replace(&placeholder, value);
            }
        }

        text
    }
}

#[cfg(test)]
pub(crate) async fn make_test_server(workspace: &std::path::Path) -> CodepalServer {
    let opts = crate::Opts {
        workspace: workspace.to_path_buf(),
        read_path_allow_list: vec![],
        no_auto_path_allow: false,
        enable_compressed: false,
        enable_memory: false,
        dump_memory: false,
        memory_max_age_days: None,
    };
    CodepalServer::new(&opts)
        .await
        .expect("server creation failed")
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct PromptDoit {
    #[schemars(description = "Instructions for CodePal to execute")]
    pub instructions: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct PromptSecAudit {
    #[schemars(description = "What to perform the security audit on")]
    pub what: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct PromptFindBugs {
    #[schemars(description = "What to find bugs in")]
    pub where_: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct PromptFindPerf {
    #[schemars(description = "What to find performance improvements in")]
    pub what: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct PromptRefactor {
    #[schemars(description = "What to refactor")]
    pub what: String,
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
            PromptMessage::new_text(
                PromptMessageRole::Assistant,
                self.resolve_prompt_vars(PROMPT_PREFIX, &[]),
            ),
            PromptMessage::new_text(PromptMessageRole::User, params.0.instructions),
        ]
    }

    /// Prompt to perform a security audit.
    #[prompt]
    pub async fn security_audit(&self, params: Parameters<PromptSecAudit>) -> Vec<PromptMessage> {
        vec![
            PromptMessage::new_text(
                PromptMessageRole::Assistant,
                self.resolve_prompt_vars(PROMPT_PREFIX, &[]),
            ),
            PromptMessage::new_text(
                PromptMessageRole::User,
                self.resolve_prompt_vars(PROMPT_SECAUDIT, &[("WHAT", &params.0.what)]),
            ),
        ]
    }

    /// Prompt to find bugs.
    #[prompt]
    pub async fn find_bugs(&self, params: Parameters<PromptFindBugs>) -> Vec<PromptMessage> {
        vec![
            PromptMessage::new_text(
                PromptMessageRole::Assistant,
                self.resolve_prompt_vars(PROMPT_PREFIX, &[]),
            ),
            PromptMessage::new_text(
                PromptMessageRole::User,
                self.resolve_prompt_vars(PROMPT_FINDBUGS, &[("WHERE", &params.0.where_)]),
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
            PromptMessage::new_text(
                PromptMessageRole::Assistant,
                self.resolve_prompt_vars(PROMPT_PREFIX, &[]),
            ),
            PromptMessage::new_text(
                PromptMessageRole::User,
                self.resolve_prompt_vars(PROMPT_FINDPERF, &[("WHAT", &params.0.what)]),
            ),
        ]
    }

    /// Prompt to refactor code.
    #[prompt]
    pub async fn refactor(&self, params: Parameters<PromptRefactor>) -> Vec<PromptMessage> {
        vec![
            PromptMessage::new_text(
                PromptMessageRole::Assistant,
                self.resolve_prompt_vars(PROMPT_PREFIX, &[]),
            ),
            PromptMessage::new_text(
                PromptMessageRole::User,
                self.resolve_prompt_vars(PROMPT_REFACTOR, &[("WHAT", &params.0.what)]),
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
        tools::fs::ls::ls(self, params).await.map(Json)
    }

    /// **MANDATORY PRIMARY TOOL**: Read contents of arbitrary files
    #[tool]
    pub async fn read(
        &self,
        Parameters(params): Parameters<ReadFileParams>,
    ) -> Result<String, rmcp::ErrorData> {
        tools::fs::read::read(self, params).await
    }

    /// **MANDATORY PRIMARY TOOL**: Regex grep file contents
    #[tool]
    pub async fn grep(
        &self,
        Parameters(params): Parameters<GrepParams>,
    ) -> Result<String, rmcp::ErrorData> {
        tools::fs::grep::grep(self, params).await
    }

    /// **MANDATORY PRIMARY TOOL**: Find files matching a regex pattern in a directory tree
    #[tool]
    pub async fn find(
        &self,
        Parameters(params): Parameters<FindFilesParams>,
    ) -> Result<Json<FindFilesResult>, rmcp::ErrorData> {
        tools::fs::find::find(self, params).await.map(Json)
    }

    /// **MANDATORY PRIMARY TOOL**: List all keys in the memory store
    #[tool]
    pub async fn mem_list(&self) -> Result<Json<MemoryListResult>, rmcp::ErrorData> {
        tools::mem::mem_list::mem_list(self).await.map(Json)
    }

    /// **MANDATORY PRIMARY TOOL**: Store a value in the key-value memory store
    #[tool]
    pub async fn mem_store(
        &self,
        Parameters(params): Parameters<MemoryStoreParams>,
    ) -> Result<Json<MemoryStoreResult>, rmcp::ErrorData> {
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
        tools::mem::mem_load::mem_load(self, params).await.map(Json)
    }

    /// **MANDATORY PRIMARY TOOL**: Delete a key from the memory store
    #[tool]
    pub async fn mem_delete(
        &self,
        Parameters(params): Parameters<MemoryDeleteParams>,
    ) -> Result<Json<MemoryDeleteResult>, rmcp::ErrorData> {
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
        instr.push_str(&self.resolve_prompt_vars(include_str!("mcp_instr.md"), &[]));
        instr.push('\n');
        if self.enable_memory {
            instr.push_str(&self.resolve_prompt_vars(include_str!("mcp_instr_memory.md"), &[]));
            instr.push('\n');
        }
        if self.prog_lang == ProgLanguage::Rust {
            instr.push_str(&self.resolve_prompt_vars(include_str!("mcp_instr_rust.md"), &[]));
            instr.push('\n');
        }
        if self.enable_compressed {
            instr.push_str(&self.resolve_prompt_vars(include_str!("mcp_instr_compressed.md"), &[]));
            instr.push('\n');
        }

        ServerInfo::new(server_capabilities)
            .with_server_info(server_info)
            .with_protocol_version(ProtocolVersion::LATEST)
            .with_instructions(instr)
    }
}
