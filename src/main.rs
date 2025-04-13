#![feature(let_chains)]
#![feature(extend_one)]

use anyhow::Result;
use clap::{Parser, Subcommand};

use nixcp::NixCp;

mod cli;
mod nixcp;
mod path_info;
mod uploader;

#[derive(Parser, Debug)]
#[command(version, name = "nixcp")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Address of the binary cache (passed to nix copy --to)
    #[arg(long, value_name = "BINARY CACHE")]
    to: String,

    /// Upstream cache to check against. Can be specified multiple times.
    /// cache.nixos.org is always included
    #[arg(long = "upstream-cache", short)]
    upstream_caches: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Push {
        /// Package or store path to upload
        /// e.g. nixpkgs#hello or /nix/store/y4qpcibkj767szhjb58i2sidmz8m24hb-hello-2.12.1
        package: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();
    let nixcp = Box::leak(Box::new(NixCp::with_upstreams(&cli.upstream_caches)?));

    match &cli.command {
        Commands::Push { package } => {
            nixcp.paths_from_package(package).await?;
            nixcp.run().await;
        }
    }

    Ok(())
}
