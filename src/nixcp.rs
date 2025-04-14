use std::{
    fs,
    iter::once,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use anyhow::{Context, Result};
use aws_config::Region;
use aws_sdk_s3 as s3;
use futures::future::join_all;
use log::{debug, info, warn};
use nix_compat::narinfo::{self, SigningKey};
use tokio::sync::{RwLock, Semaphore, mpsc};
use url::Url;

use crate::{Cli, path_info::PathInfo, uploader::Uploader};

pub struct NixCp {
    upstream_caches: Arc<Vec<Url>>,
    store_paths: Arc<RwLock<Vec<PathInfo>>>,
    s3_client: s3::Client,
    signing_key: SigningKey<ed25519_dalek::SigningKey>,
    bucket: String,
}

impl NixCp {
    pub async fn new(cli: &Cli) -> Result<Self> {
        let mut upstreams = Vec::with_capacity(cli.upstreams.len() + 1);
        for upstream in cli
            .upstreams
            .iter()
            .chain(once(&"https://cache.nixos.org".to_string()))
        {
            upstreams
                .push(Url::parse(upstream).context(format!("failed to parse {upstream} as url"))?);
        }

        let key = fs::read_to_string(&cli.signing_key)?;
        let signing_key = narinfo::parse_keypair(key.as_str())?.0;

        let mut s3_config = aws_config::from_env();
        if let Some(region) = &cli.region {
            s3_config = s3_config.region(Region::new(region.clone()));
        }
        if let Some(endpoint) = &cli.endpoint {
            s3_config = s3_config.endpoint_url(endpoint);
        }
        if let Some(profile) = &cli.profile {
            s3_config = s3_config.profile_name(profile);
        }

        let s3_client = s3::Client::new(&s3_config.load().await);
        Ok(Self {
            upstream_caches: Arc::new(upstreams),
            store_paths: Arc::new(RwLock::new(Vec::new())),
            s3_client,
            signing_key,
            bucket: cli.bucket.clone(),
        })
    }

    pub async fn paths_from_package(&mut self, package: &str) -> Result<()> {
        let path_info = PathInfo::from_path(package)
            .await
            .context("get path info for package")?;
        debug!("path-info for {package}: {:?}", path_info);
        self.store_paths.write().await.extend(
            path_info
                .get_closure()
                .await
                .context("closure from path info")?,
        );
        info!("found {} store paths", self.store_paths.read().await.len());

        Ok(())
    }

    pub async fn run(&'static self) -> Result<()> {
        let (tx, rx) = mpsc::channel(10);
        let tx = Arc::new(tx);
        tokio::spawn(self.filter_from_upstream(tx));
        self.upload(rx).await
    }

    /// filter paths that are on upstream and send to `tx`
    async fn filter_from_upstream(&self, tx: Arc<mpsc::Sender<PathInfo>>) {
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
                        tx.send(path).await.unwrap();
                    }
                })
            });
        }

        for handle in handles {
            handle.await.unwrap();
        }
    }

    async fn upload(&'static self, mut rx: mpsc::Receiver<PathInfo>) -> Result<()> {
        let upload_count = AtomicUsize::new(0);
        let failures: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let permits = Arc::new(Semaphore::new(10));
        let mut uploads = Vec::with_capacity(10);

        loop {
            if let Some(path_to_upload) = rx.recv().await {
                let permits = Arc::clone(&permits);
                let absolute_path = path_to_upload.absolute_path();

                info!("uploading: {}", absolute_path);
                let uploader = Uploader::new(
                    &self.signing_key,
                    path_to_upload,
                    &self.s3_client,
                    self.bucket.clone(),
                )?;

                let fut = tokio::spawn({
                    let _permit = permits.acquire().await.unwrap();
                    async move { uploader.upload().await }
                });
                uploads.push(fut);
            } else {
                join_all(uploads).await;
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
        Ok(())
    }
}
