use anyhow::Context as _;
use clap::Parser;
use rmcp::{ServiceExt, transport::stdio};
use std::path::PathBuf;

mod mcp;
mod mcp_struct;

#[derive(Parser, Debug)]
#[command(author, version, about = "Codepal MCP server")]
pub struct Opts {
    /// Path to the project workspace root.
    #[arg(long, short = 'w', value_name = "PATH")]
    workspace: PathBuf,

    /// Allowed path prefixes. Only files under these paths may be read.
    /// May be specified multiple times.
    #[arg(long = "allow-read", short = 'r', value_name = "PATH")]
    read_path_allow_list: Vec<PathBuf>,

    /// Disable automatic language-specific path additions (e.g. ~/.cargo, ~/.rustup for Rust).
    #[arg(long)]
    no_auto_path_allow: bool,

    /// Enable compressed communication.
    #[arg(long, short = 'C')]
    enable_compressed: bool,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts = Opts::parse();
    let server = mcp::CodepalServer::new(&opts)
        .await
        .context("Start MCP server")?;
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
