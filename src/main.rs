#![feature(let_chains)]

use std::path::Path;
use std::sync::mpsc;
use std::sync::{
    Arc, Mutex, RwLock,
    atomic::{AtomicUsize, Ordering},
};

use clap::Parser;
use log::{debug, trace};
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::sync::Semaphore;

const UPSTREAM_CACHES: &[&str] = &["https://cache.nixos.org"];

// nix path-info --derivation --json
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathInfo {
    ca: String,
    nar_hash: String,
    nar_size: u32,
    path: String,
    references: Vec<String>,
    registration_time: u32,
    valid: bool,
}

impl PathInfo {
    // find derivations related to package
    async fn from_package(package: &str) -> Vec<Self> {
        let path_infos = Command::new("nix")
            .arg("path-info")
            .arg("--derivation")
            .arg("--recursive")
            .arg("--json")
            .arg(package)
            .output()
            .await
            .expect("path-info failed");

        let path_infos: Vec<PathInfo> = serde_json::from_slice(&path_infos.stdout).unwrap();
        debug!("PathInfo's from nix path-info: {:#?}", path_infos);
        path_infos
    }

    // find store paths related to derivation
    async fn get_store_paths(&self) -> Vec<String> {
        let mut store_paths: Vec<String> = Vec::new();
        let nix_store_cmd = Command::new("nix-store")
            .arg("--query")
            .arg("--requisites")
            .arg("--include-outputs")
            .arg(&self.path)
            .output()
            .await
            .expect("nix-store cmd failed");

        let nix_store_out = String::from_utf8(nix_store_cmd.stdout).unwrap();
        for store_path in nix_store_out.split_whitespace().map(ToString::to_string) {
            store_paths.push(store_path);
        }
        store_paths
    }
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Package to upload to the binary cache
    package: String,

    /// Address of the binary cache (passed to nix copy --to)
    #[arg(long, value_name = "BINARY CACHE")]
    to: String,

    /// Upstream cache to check against. Can be specified multiple times.
    /// cache.nixos.org is always included
    #[arg(long, short)]
    upstream_cache: Vec<String>,

    /// Concurrent upstream cache checkers
    #[arg(long, default_value_t = 50)]
    upstream_checker_concurrency: u8,

    /// Concurrent uploaders
    #[arg(long, default_value_t = 10)]
    uploader_concurrency: u8,

    /// Concurrent nix-store commands to run
    #[arg(long, default_value_t = 50)]
    nix_store_concurrency: u8,
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let cli = Cli::parse();
    let package = &cli.package;
    let binary_cache = cli.to;
    let mut upstream_caches = cli.upstream_cache;
    for upstream in UPSTREAM_CACHES {
        upstream_caches.push(upstream.to_string());
    }
    debug!("package: {}", package);
    debug!("binary cache: {}", binary_cache);
    debug!("upstream caches: {:#?}", upstream_caches);

    println!("querying nix path-info");
    let derivations = PathInfo::from_package(package).await;
    println!("got {} derivations", derivations.len());

    println!("querying nix-store");
    let mut handles = Vec::new();
    let concurrency = Arc::new(Semaphore::new(cli.nix_store_concurrency.into()));
    let store_paths = Arc::new(RwLock::new(Vec::new()));

    for derivation in derivations {
        let store_paths = Arc::clone(&store_paths);
        let permit = Arc::clone(&concurrency);
        handles.push(tokio::spawn(async move {
            let _permit = permit.acquire_owned().await.unwrap();
            let paths = derivation.get_store_paths().await;
            store_paths.write().unwrap().extend(paths);
        }));
    }
    // resolve store paths for all derivations before we move on
    for handle in handles {
        handle.await.unwrap();
    }
    println!("got {} store paths", store_paths.read().unwrap().len());

    let (cacheable_tx, cacheable_rx) = mpsc::channel();

    println!("spawning check_upstream");
    handles = Vec::new();
    handles.push(tokio::spawn(async move {
        check_upstream(
            store_paths,
            cacheable_tx,
            cli.upstream_checker_concurrency,
            Arc::new(upstream_caches),
        )
        .await;
    }));

    println!("spawning uploader");
    handles.push(tokio::spawn(async move {
        uploader(cacheable_rx, binary_cache, cli.uploader_concurrency).await;
    }));

    // make sure all threads are done
    for handle in handles {
        handle.await.unwrap();
    }
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
                tx.send(store_path).unwrap();
            }
        });
    }
}

async fn uploader(cacheable_rx: mpsc::Receiver<String>, binary_cache: String, concurrency: u8) {
    let upload_count = Arc::new(AtomicUsize::new(0));
    let failures: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let concurrency = Arc::new(Semaphore::new(concurrency.into()));
    let mut handles = Vec::new();

    loop {
        if let Ok(path_to_upload) = cacheable_rx.recv() {
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
