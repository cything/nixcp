use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

mod bindings;
mod cli;
pub mod make_nar;
pub mod path_info;
pub mod push;
pub mod store;
mod uploader;

#[derive(Parser, Debug)]
#[command(version)]
#[command(name = "nixcp")]
#[command(about = "Upload store paths to a s3 binary cache")]
#[command(long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Whether to enable tokio-console
    #[arg(long)]
    pub tokio_console: bool,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
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
    pub paths: Vec<PathBuf>,
}
