use std::{
    fs,
    iter::once,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use anyhow::{Context, Result};
use futures::future::join_all;
use nix_compat::narinfo::{self, SigningKey};
use object_store::aws::{AmazonS3, AmazonS3Builder};
use tokio::sync::{RwLock, Semaphore, mpsc};
use tracing::debug;
use url::Url;

use crate::{PushArgs, path_info::PathInfo, store::Store, uploader::Uploader};

pub struct Push {
    upstream_caches: Vec<Url>,
    store_paths: Arc<RwLock<Vec<PathInfo>>>,
    signing_key: SigningKey<ed25519_dalek::SigningKey>,
    store: Arc<Store>,
    s3: Arc<AmazonS3>,
    // paths that we skipped cause of a signature match
    signature_hit_count: AtomicUsize,
    // paths that we skipped cause we found it on an upstream
    upstream_hit_count: AtomicUsize,
    // paths that we skipped cause they are already on our cache
    already_exists_count: AtomicUsize,
    // paths that we uploaded
    upload_count: AtomicUsize,
}

impl Push {
    pub async fn new(cli: &PushArgs, store: Store) -> Result<Self> {
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

        let mut s3_builder = AmazonS3Builder::from_env().with_bucket_name(&cli.bucket);

        if let Some(region) = &cli.region {
            s3_builder = s3_builder.with_region(region);
        }
        if let Some(endpoint) = &cli.endpoint {
            s3_builder = s3_builder.with_endpoint(endpoint);
        }

        Ok(Self {
            upstream_caches: upstreams,
            store_paths: Arc::new(RwLock::new(Vec::new())),
            signing_key,
            store: Arc::new(store),
            s3: Arc::new(s3_builder.build()?),
            signature_hit_count: AtomicUsize::new(0),
            upstream_hit_count: AtomicUsize::new(0),
            already_exists_count: AtomicUsize::new(0),
            upload_count: AtomicUsize::new(0),
        })
    }

    pub async fn add_paths(&'static self, paths: Vec<PathBuf>) -> Result<()> {
        let mut futs = Vec::with_capacity(paths.len());
        for path in paths {
            let store_paths = self.store_paths.clone();
            let store = self.store.clone();

            futs.push(tokio::spawn(async move {
                let path_info = PathInfo::from_path(path.as_path(), &store)
                    .await
                    .context("get path info for path")?;
                debug!("path-info for {path:?}: {path_info:?}");

                store_paths.write().await.extend(
                    path_info
                        .get_closure(&store)
                        .await
                        .context("closure from path info")?,
                );
                Ok(())
            }));
        }
        join_all(futs)
            .await
            .into_iter()
            .flatten()
            .collect::<Result<Vec<_>>>()?;
        println!("found {} store paths", self.store_paths.read().await.len());

        Ok(())
    }

    pub async fn run(&'static self) -> Result<()> {
        let (tx, rx) = mpsc::channel(1);
        let filter = tokio::spawn(self.filter_from_upstream(tx));
        let upload = tokio::spawn(self.upload(rx));

        filter.await?;
        upload.await??;
        Ok(())
    }

    /// filter paths that are on upstream and send to `tx`
    async fn filter_from_upstream(&'static self, tx: mpsc::Sender<PathInfo>) {
        let mut handles = Vec::new();
        let store_paths = self.store_paths.read().await.clone();
        // limit number of inflight requests
        let inflight_permits = Arc::new(Semaphore::new(32));

        for path in store_paths.into_iter() {
            if path.check_upstream_signature(&self.upstream_caches) {
                debug!("skip {} (signature match)", path.absolute_path());
                self.signature_hit_count.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            handles.push({
                let tx = tx.clone();
                let inflight_permits = inflight_permits.clone();
                tokio::spawn(async move {
                    let _permit = inflight_permits.acquire().await.unwrap();
                    if !path
                        .check_upstream_hit(self.upstream_caches.as_slice())
                        .await
                    {
                        if path.check_if_already_exists(&self.s3).await {
                            debug!("skip {} (already exists)", path.absolute_path());
                            self.already_exists_count.fetch_add(1, Ordering::Relaxed);
                        } else {
                            tx.send(path).await.unwrap();
                        }
                    } else {
                        debug!("skip {} (upstream hit)", path.absolute_path());
                        self.upstream_hit_count.fetch_add(1, Ordering::Relaxed);
                    }
                })
            });
        }

        join_all(handles)
            .await
            .into_iter()
            .collect::<std::result::Result<(), _>>()
            .unwrap();
    }

    async fn upload(&'static self, mut rx: mpsc::Receiver<PathInfo>) -> Result<()> {
        let mut uploads = Vec::new();
        let permits = Arc::new(Semaphore::new(16));
        let big_permits = Arc::new(Semaphore::new(5));

        loop {
            let permits = permits.clone();
            let big_permits = big_permits.clone();

            if let Some(path_to_upload) = rx.recv().await {
                debug!("upload permits available: {}", permits.available_permits());
                let mut permit = permits.acquire_owned().await.unwrap();

                uploads.push(tokio::spawn({
                    // a large directory may have many files and end up causing "too many open files"
                    if PathBuf::from(path_to_upload.absolute_path()).is_dir()
                        && path_to_upload.nar_size > 5 * 1024 * 1024
                    {
                        debug!(
                            "upload big permits available: {}",
                            big_permits.available_permits()
                        );
                        // drop regular permit and take the big one
                        permit = big_permits.acquire_owned().await.unwrap();
                    }

                    println!(
                        "uploading: {} (size: {})",
                        path_to_upload.absolute_path(),
                        path_to_upload.nar_size
                    );
                    let uploader = Uploader::new(&self.signing_key, path_to_upload)?;
                    let s3 = self.s3.clone();
                    let store = self.store.clone();
                    async move {
                        let res = uploader.upload(s3, store).await;
                        drop(permit);
                        self.upload_count.fetch_add(1, Ordering::Relaxed);
                        res
                    }
                }));
            } else {
                join_all(uploads)
                    .await
                    .into_iter()
                    .flatten()
                    .collect::<Result<Vec<_>>>()?;

                println!("uploaded: {}", self.upload_count.load(Ordering::Relaxed));
                println!(
                    "skipped because of signature match: {}",
                    self.signature_hit_count.load(Ordering::Relaxed)
                );
                println!(
                    "skipped because of upstream hit: {}",
                    self.upstream_hit_count.load(Ordering::Relaxed)
                );
                println!(
                    "skipped because already exist: {}",
                    self.already_exists_count.load(Ordering::Relaxed)
                );
                break;
            }
        }
        Ok(())
    }
}
