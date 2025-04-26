#![feature(let_chains)]
#![feature(extend_one)]
#![feature(exit_status_error)]

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};

use push::Push;
use store::Store;

mod bindings;
mod cli;
mod make_nar;
mod path_info;
mod push;
mod store;
mod uploader;

#[derive(Parser, Debug)]
#[command(version)]
#[command(name = "nixcp")]
#[command(about = "Upload store paths to a s3 binary cache")]
#[command(long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(arg_required_else_help = true)]
    Push(PushArgs),
}

#[derive(Debug, Args)]
pub struct PushArgs {
    /// The s3 bucket to upload to
    #[arg(long, value_name = "bucket name")]
    bucket: String,

    /// Upstream cache to check against. Can be specified multiple times.
    /// cache.nixos.org is always included.
    #[arg(long = "upstream", short, value_name = "nixcache.example.com")]
    upstreams: Vec<String>,

    /// Path to the file containing signing key
    /// e.g. ~/cache-priv-key.pem
    #[arg(long)]
    signing_key: String,

    /// If unspecified, will get it form AWS_DEFAULT_REGION envar or default to us-east-1
    #[arg(long)]
    region: Option<String>,

    /// If unspecifed, will get it from AWS_ENDPOINT envar
    /// e.g. https://s3.example.com
    #[arg(long)]
    endpoint: Option<String>,

    #[arg(long)]
    skip_signature_check: bool,

    /// Path to upload
    /// e.g. ./result or /nix/store/y4qpcibkj767szhjb58i2sidmz8m24hb-hello-2.12.1
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    console_subscriber::init();

    let cli = Cli::parse();

    match &cli.command {
        Commands::Push(cli) => {
            let store = Store::connect()?;
            let push = Box::leak(Box::new(Push::new(cli, store).await?));
            push.add_paths(cli.paths.clone())
                .await
                .context("add paths to push")?;
            push.run().await.context("nixcp run")?;
        }
    }

    Ok(())
}
