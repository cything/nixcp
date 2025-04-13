use std::sync::Arc;

use crate::path_info::PathInfo;
use anyhow::{Context, Result};
use log::info;
use tokio::sync::{Semaphore, mpsc};
use url::Url;

pub struct NixCp {
    upstream_caches: Arc<Vec<Url>>,
    store_paths: Vec<PathInfo>,
}

impl NixCp {
    pub fn new() -> Self {
        Self {
            upstream_caches: vec![Url::parse("https://cache.nixos.org").unwrap()],
            store_paths: Vec::new(),
        }
    }

    pub fn add_upstreams(&mut self, upstreams: &[String]) -> Result<()> {
        self.upstream_caches.reserve(upstreams.len());
        for upstream in upstreams {
            self.upstream_caches
                .push(Url::parse(upstream).context(format!("failed to parse {upstream} as url"))?);
        }
        Ok(())
    }

    pub async fn paths_from_package(&mut self, package: &str) -> Result<()> {
        let path_info = PathInfo::from_path(package).await?;
        self.store_paths = path_info.get_closure().await?;
        info!("found {} store paths", self.store_paths.len());

        Ok(())
    }

    pub async fn run(&mut self) {}

    /// filter paths that are on upstream and send to `tx`
    async fn filter_from_upstream(&self, tx: mpsc::Sender<&PathInfo>) {
        let permits = Arc::new(Semaphore::new(10));
        let mut handles = Vec::new();
        for path in &self.store_paths {
            if path.check_upstream_signature(&self.upstream_caches) {
                continue;
            }
            let permits = permits.clone();
            let tx = tx.clone();
            handles.push(tokio::spawn(async move {
                let _permit = permits.acquire().await.unwrap();

                if !path.check_upstream_hit(&self.upstream_caches).await {
                    tx.send(path);
                }
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }
    }
}
