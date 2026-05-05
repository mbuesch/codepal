use anyhow::Context as _;
use clap::Parser;
use rmcp::{ServiceExt, transport::stdio};
use std::path::PathBuf;

mod mcp;

#[derive(Parser, Debug)]
#[command(author, version, about = "Codepal MCP server")]
pub struct Opts {
    /// Path to the project workspace root.
    #[arg(long, short = 'w', value_name = "PATH", default_value = ".")]
    pub workspace: PathBuf,

    /// Allowed path prefixes. Only files under these paths may be read.
    /// May be specified multiple times.
    #[arg(long = "allow-read", short = 'r', value_name = "PATH")]
    pub read_path_allow_list: Vec<PathBuf>,

    /// Disable automatic language-specific path additions (e.g. ~/.cargo, ~/.rustup for Rust).
    #[arg(long)]
    pub no_auto_path_allow: bool,

    /// Enable compressed communication.
    #[arg(long, short = 'C')]
    pub enable_compressed: bool,

    /// Dump memory store contents to stdout and exit.
    #[arg(long, short = 'D')]
    pub dump_memory: bool,

    /// Prune memory entries not accessed for longer than this many days.
    #[arg(long, value_name = "DAYS")]
    pub memory_max_age_days: Option<u64>,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts = Opts::parse();
    let server = mcp::CodepalServer::new(&opts)
        .await
        .context("Start MCP server")?;
    if opts.dump_memory {
        server.dump_memory().await?;
        return Ok(());
    }
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
