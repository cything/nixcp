#![feature(let_chains)]
#![feature(extend_one)]

use std::path::Path;
use std::sync::{
    Arc, Mutex, RwLock,
    atomic::{AtomicUsize, Ordering},
};

use anyhow::Result;
use clap::{Parser, Subcommand};
use log::{debug, trace};
use tokio::process::Command;
use tokio::sync::{Semaphore, mpsc};

use nixcp::NixCp;

mod cli;
mod nixcp;
mod path_info;

#[derive(Parser, Debug)]
#[command(version, name = "nixcp")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Address of the binary cache (passed to nix copy --to)
    #[arg(long, value_name = "BINARY CACHE")]
    to: String,

    /// Upstream cache to check against. Can be specified multiple times.
    /// cache.nixos.org is always included (unless --no-nixos-cache is passed)
    #[arg(long = "upstream-cache", short)]
    upstream_caches: Vec<String>,

    /// Concurrent upstream cache checkers
    #[arg(long, default_value_t = 32)]
    upstream_checker_concurrency: u8,

    /// Concurrent uploaders
    #[arg(long, default_value_t = 4)]
    uploader_concurrency: u8,

    /// Concurrent nix-store commands to run
    #[arg(long, default_value_t = 32)]
    nix_store_concurrency: u8,
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
    let mut nixcp = NixCp::new();
    nixcp.add_upstreams(&cli.upstream_caches)?;

    match &cli.command {
        Commands::Push { package } => {
            nixcp.paths_from_package(package).await?;
        }
    }

    Ok(())

    /*
        let (cacheable_tx, mut cacheable_rx) = mpsc::channel(cli.uploader_concurrency.into());

        println!("spawning check_upstream");

        println!("spawning uploader");
        handles.push(tokio::spawn(async move {
            uploader(&mut cacheable_rx, binary_cache, cli.uploader_concurrency).await;
        }));

        // make sure all threads are done
        for handle in handles {
            handle.await.unwrap();
        }
    */
}

// filter out store paths that exist in upstream caches
async fn check_upstream(
    store_paths: Arc<RwLock<Vec<String>>>,
    cacheable_tx: mpsc::Sender<String>,
    concurrency: u8,
    upstream_caches: Arc<Vec<String>>,
) {
    let concurrency = Arc::new(Semaphore::new(concurrency.into()));
    let c_store_paths = Arc::clone(&store_paths);
    let store_paths = c_store_paths.read().unwrap().clone();

    for store_path in store_paths {
        let tx = cacheable_tx.clone();
        let upstream_caches = Arc::clone(&upstream_caches);
        let concurrency = Arc::clone(&concurrency);

        tokio::spawn(async move {
            let _permit = concurrency.acquire().await.unwrap();
            let basename = Path::new(&store_path)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string();
            let hash = basename.split("-").next().unwrap();

            let mut hit = false;
            for upstream in upstream_caches.as_ref() {
                let mut uri = upstream.clone();
                uri.push_str(format!("/{}.narinfo", hash).as_str());

                let res_status = reqwest::Client::new()
                    .head(uri)
                    .send()
                    .await
                    .map(|x| x.status());

                if let Ok(res_status) = res_status
                    && res_status.is_success()
                {
                    debug!("{} was a hit upstream: {}", store_path, upstream);
                    hit = true;
                    break;
                }
            }
            if !hit {
                trace!("sending {}", store_path);
                tx.send(store_path).await.unwrap();
            }
        });
    }
}

async fn uploader(
    cacheable_rx: &mut mpsc::Receiver<String>,
    binary_cache: String,
    concurrency: u8,
) {
    let upload_count = Arc::new(AtomicUsize::new(0));
    let failures: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let concurrency = Arc::new(Semaphore::new(concurrency.into()));
    let mut handles = Vec::new();

    loop {
        if let Some(path_to_upload) = cacheable_rx.recv().await {
            let concurrency = Arc::clone(&concurrency);
            let failures = Arc::clone(&failures);
            let binary_cache = binary_cache.clone();
            let upload_count = Arc::clone(&upload_count);

            handles.push(tokio::spawn(async move {
                let _permit = concurrency.acquire().await.unwrap();
                println!("uploading: {}", path_to_upload);
                if Command::new("nix")
                    .arg("copy")
                    .arg("--to")
                    .arg(&binary_cache)
                    .arg(&path_to_upload)
                    .output()
                    .await
                    .is_err()
                {
                    println!("WARN: upload failed: {}", path_to_upload);
                    failures.lock().unwrap().push(path_to_upload);
                } else {
                    upload_count.fetch_add(1, Ordering::Relaxed);
                }
            }));
        } else {
            // make sure all threads are done
            for handle in handles {
                handle.await.unwrap();
            }
            println!("uploaded {} paths", upload_count.load(Ordering::Relaxed));

            let failures = failures.lock().unwrap();
            if !failures.is_empty() {
                println!("failed to upload these paths: ");
                for failure in failures.iter() {
                    print!("{}", failure);
                }
                println!();
            }
            break;
        }
    }
}
