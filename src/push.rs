use std::{
    fs,
    iter::once,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use anyhow::{Context, Result};
use aws_config::Region;
use aws_sdk_s3 as s3;
use futures::future::join_all;
use nix_compat::narinfo::{self, SigningKey};
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, info, trace};
use url::Url;

use crate::{PushArgs, path_info::PathInfo, uploader::Uploader};

pub struct Push {
    upstream_caches: Vec<Url>,
    store_paths: Arc<RwLock<Vec<PathInfo>>>,
    s3_client: s3::Client,
    signing_key: SigningKey<ed25519_dalek::SigningKey>,
    bucket: String,
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
    pub async fn new(cli: &PushArgs) -> Result<Self> {
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
            upstream_caches: upstreams,
            store_paths: Arc::new(RwLock::new(Vec::new())),
            s3_client,
            signing_key,
            bucket: cli.bucket.clone(),
            signature_hit_count: AtomicUsize::new(0),
            upstream_hit_count: AtomicUsize::new(0),
            already_exists_count: AtomicUsize::new(0),
            upload_count: AtomicUsize::new(0),
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
        let filter = tokio::spawn(self.filter_from_upstream(tx));
        let upload = tokio::spawn(self.upload(rx));
        filter.await?;
        upload.await??;
        Ok(())
    }

    /// filter paths that are on upstream and send to `tx`
    async fn filter_from_upstream(&'static self, tx: mpsc::Sender<PathInfo>) {
        let mut handles = Vec::with_capacity(10);
        let store_paths = self.store_paths.read().await.clone();

        for path in store_paths.into_iter() {
            if path.check_upstream_signature(&self.upstream_caches) {
                trace!("skip {} (signature match)", path.absolute_path());
                self.signature_hit_count.fetch_add(1, Ordering::Release);
                continue;
            }
            handles.push({
                let tx = tx.clone();
                tokio::spawn(async move {
                    if !path
                        .check_upstream_hit(self.upstream_caches.as_slice())
                        .await
                    {
                        if path
                            .check_if_already_exists(&self.s3_client, self.bucket.clone())
                            .await
                        {
                            trace!("skip {} (already exists)", path.absolute_path());
                            self.already_exists_count.fetch_add(1, Ordering::Relaxed);
                        } else {
                            tx.send(path).await.unwrap();
                        }
                    } else {
                        trace!("skip {} (upstream hit)", path.absolute_path());
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
        let mut uploads = Vec::with_capacity(10);

        loop {
            if let Some(path_to_upload) = rx.recv().await {
                let absolute_path = path_to_upload.absolute_path();

                println!("uploading: {}", absolute_path);
                let uploader = Uploader::new(
                    &self.signing_key,
                    path_to_upload,
                    &self.s3_client,
                    self.bucket.clone(),
                )?;

                uploads.push(tokio::spawn(async move {
                    let res = uploader.upload().await;
                    self.upload_count.fetch_add(1, Ordering::Relaxed);
                    res
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
