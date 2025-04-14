#![feature(let_chains)]
#![feature(extend_one)]

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use push::Push;

mod cli;
mod path_info;
mod push;
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

    /// If unspecified, will get it form AWS_DEFAULT_REGION envar
    #[arg(long)]
    region: Option<String>,

    /// If unspecifed, will get it from AWS_ENDPOINT_URL envar
    /// e.g. https://s3.example.com
    #[arg(long)]
    endpoint: Option<String>,

    /// AWS profile to use
    #[arg(long)]
    profile: Option<String>,

    #[arg(long)]
    skip_signature_check: bool,

    /// Package or store path to upload
    /// e.g. nixpkgs#hello or /nix/store/y4qpcibkj767szhjb58i2sidmz8m24hb-hello-2.12.1
    #[arg(value_name = "package or store path")]
    package: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let filter = EnvFilter::from_default_env();
    let subscriber = FmtSubscriber::builder().with_env_filter(filter).finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let cli = Cli::parse();

    match &cli.command {
        Commands::Push(cli) => {
            let push = Box::leak(Box::new(Push::new(cli).await?));
            push.paths_from_package(&cli.package)
                .await
                .context("nixcp get paths from package")?;
            push.run().await.context("nixcp run")?;
        }
    }

    Ok(())
}
