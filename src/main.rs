#![feature(let_chains)]
#![feature(extend_one)]
#![feature(array_chunks)]

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

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

    /// The s3 bucket to upload to
    #[arg(long, value_name = "bucket name")]
    bucket: String,

    /// Upstream cache to check against. Can be specified multiple times.
    /// cache.nixos.org is always included
    #[arg(long = "upstream", short, value_name = "nixcache.example.com")]
    upstreams: Vec<String>,

    /// Path to the file containing signing key
    /// e.g. ~/cache-priv-key.pem
    #[arg(long)]
    signing_key: String,

    /// If unspecified, will get it form AWS_DEFAULT_REGION envar or the AWS default
    #[arg(long)]
    region: Option<String>,

    /// If unspecifed, will get it from AWS_ENDPOINT_URL envar or the AWS default
    /// e.g. s3.example.com
    #[arg(long)]
    endpoint: Option<String>,

    /// AWS profile to use
    #[arg(long)]
    profile: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Push {
        /// Package or store path to upload
        /// e.g. nixpkgs#hello or /nix/store/y4qpcibkj767szhjb58i2sidmz8m24hb-hello-2.12.1
        #[arg(value_name = "package or store path")]
        package: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let filter = EnvFilter::from_default_env();
    let subscriber = FmtSubscriber::builder().with_env_filter(filter).finish();
    tracing::subscriber::set_global_default(subscriber)?;
    let cli = Cli::parse();
    let nixcp = Box::leak(Box::new(NixCp::new(&cli).await?));

    match &cli.command {
        Commands::Push { package } => {
            nixcp
                .paths_from_package(package)
                .await
                .context("nixcp get paths from package")?;
            nixcp.run().await.context("nixcp run")?;
        }
    }

    Ok(())
}
