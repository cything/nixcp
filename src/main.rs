#![feature(let_chains)]
#![feature(exit_status_error)]

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use tracing_subscriber::{EnvFilter, prelude::*};

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

    /// Whether to enable tokio-console
    #[arg(long)]
    tokio_console: bool,
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
    let cli = Cli::parse();
    init_logging(cli.tokio_console);

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

fn init_logging(tokio_console: bool) {
    let env_filter = EnvFilter::from_default_env();
    let fmt_layer = tracing_subscriber::fmt::layer().with_filter(env_filter);

    let console_layer = if tokio_console {
        Some(console_subscriber::spawn())
    } else {
        None
    };

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(console_layer)
        .init();

    if tokio_console {
        println!("tokio-console is enabled");
    }
}
