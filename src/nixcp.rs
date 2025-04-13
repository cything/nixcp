use std::{
    iter::once,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use crate::path_info::PathInfo;
use anyhow::{Context, Result};
use log::{info, warn};
use tokio::{
    process::Command,
    sync::{RwLock, Semaphore, mpsc},
};
use url::Url;

pub struct NixCp {
    upstream_caches: Arc<Vec<Url>>,
    store_paths: Arc<RwLock<Vec<PathInfo>>>,
}

impl NixCp {
    pub fn with_upstreams(new_upstreams: &[String]) -> Result<Self> {
        let mut upstreams = Vec::with_capacity(new_upstreams.len() + 1);
        for upstream in new_upstreams
            .iter()
            .chain(once(&"https://cache.nixos.org".to_string()))
        {
            upstreams
                .push(Url::parse(upstream).context(format!("failed to parse {upstream} as url"))?);
        }
        Ok(Self {
            upstream_caches: Arc::new(upstreams),
            store_paths: Arc::new(RwLock::new(Vec::new())),
        })
    }

    pub async fn paths_from_package(&mut self, package: &str) -> Result<()> {
        let path_info = PathInfo::from_path(package).await?;
        self.store_paths
            .write()
            .await
            .extend(path_info.get_closure().await?);
        info!("found {} store paths", self.store_paths.read().await.len());

        Ok(())
    }

    pub async fn run(&'static self) {
        let (tx, rx) = mpsc::channel(10);
        let tx = Arc::new(tx);
        tokio::spawn(self.filter_from_upstream(tx));
        tokio::spawn(self.uploader("".to_string(), rx));
    }

    /// filter paths that are on upstream and send to `tx`
    async fn filter_from_upstream(&self, tx: Arc<mpsc::Sender<String>>) {
        let permits = Arc::new(Semaphore::new(10));
        let mut handles = Vec::with_capacity(10);
        let store_paths = self.store_paths.read().await.clone();

        for path in store_paths.into_iter() {
            if path.check_upstream_signature(&self.upstream_caches) {
                continue;
            }
            handles.push({
                let permits = permits.clone();
                let tx = tx.clone();
                let upstream_caches = self.upstream_caches.clone();
                tokio::spawn(async move {
                    let _permit = permits.acquire().await.unwrap();

                    if !path.check_upstream_hit(upstream_caches.as_slice()).await {
                        tx.send(path.to_string()).await.unwrap();
                    }
                })
            });
        }

        for handle in handles {
            handle.await.unwrap();
        }
    }

    async fn uploader(&self, cache: String, mut rx: mpsc::Receiver<String>) {
        let upload_count = Arc::new(AtomicUsize::new(0));
        let failures: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let permits = Arc::new(Semaphore::new(10));
        let mut handles = Vec::with_capacity(10);

        loop {
            if let Some(path_to_upload) = rx.recv().await {
                let permits = Arc::clone(&permits);
                let failures = Arc::clone(&failures);
                let binary_cache = cache.clone();
                let upload_count = Arc::clone(&upload_count);

                handles.push(tokio::spawn(async move {
                    let _permit = permits.acquire().await.unwrap();
                    info!("uploading: {}", path_to_upload.to_string());
                    if Command::new("nix")
                        .arg("copy")
                        .arg("--to")
                        .arg(&binary_cache)
                        .arg(&path_to_upload.to_string())
                        .output()
                        .await
                        .is_err()
                    {
                        warn!("upload failed: {}", path_to_upload);
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
                    warn!("failed to upload these paths: ");
                    for failure in failures.iter() {
                        warn!("{}", failure);
                    }
                }
                break;
            }
        }
    }
}
