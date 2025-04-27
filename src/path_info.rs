use std::collections::HashSet;

use anyhow::{Context, Result};
use futures::future::join_all;
use nix_compat::nixbase32;
use nix_compat::store_path::StorePath;
use object_store::{ObjectStore, aws::AmazonS3, path::Path as ObjectPath};
use regex::Regex;
use std::path::Path;
use tokio::process::Command;
use tracing::{debug, trace};
use url::Url;

use crate::store::Store;

#[derive(Debug, Clone)]
pub struct PathInfo {
    pub path: StorePath<String>,
    pub signatures: Vec<String>,
    pub references: Vec<StorePath<String>>,
    pub nar_size: u64,
}

impl PathInfo {
    pub async fn from_path(path: &Path, store: &Store) -> Result<Self> {
        debug!("query path info for {:?}", path);

        let derivation = match path.extension() {
            Some(ext) if ext == "drv" => path.as_os_str().as_encoded_bytes(),
            _ => {
                &Command::new("nix")
                    .arg("path-info")
                    .arg("--derivation")
                    .arg(path)
                    .output()
                    .await
                    .context(format!("run command: nix path-info --derivaiton {path:?}"))?
                    .stdout
            }
        };
        let derivation = String::from_utf8_lossy(derivation);
        debug!("derivation: {derivation}");

        let store_path = StorePath::from_absolute_path(derivation.trim().as_bytes())
            .context("storepath from derivation")?;
        store
            .query_path_info(store_path)
            .await
            .context("query pathinfo for derivation")
    }

    pub async fn get_closure(&self, store: &Store) -> Result<Vec<Self>> {
        let futs = store
            .compute_fs_closure(self.path.clone())
            .await?
            .into_iter()
            .map(|x| store.query_path_info(x));
        join_all(futs).await.into_iter().collect()
    }

    /// checks if the path is signed by any upstream. if it is, we assume a cache hit.
    /// the name of the cache in the signature does not have to be the domain of the cache.
    /// in fact, it can be any random string. but, most often it is, and this saves us
    /// a request.
    pub fn check_upstream_signature(&self, upstreams: &[Url]) -> bool {
        let upstreams: HashSet<_> = upstreams.iter().filter_map(|x| x.domain()).collect();

        // some caches use names prefixed with -<some number>
        // e.g. cache.nixos.org-1, nix-community.cachix.org-1
        let re = Regex::new(r"-\d+$").expect("regex should be valid");
        for signee in self.signees().iter().map(|&x| re.replace(x, "")) {
            if upstreams.contains(signee.as_ref()) {
                return true;
            }
        }
        false
    }

    fn signees(&self) -> Vec<&str> {
        let signers: Vec<_> = self
            .signatures
            .iter()
            .filter_map(|signature| Some(signature.split_once(":")?.0))
            .collect();
        trace!("signers for {}: {:?}", self.path, signers);
        signers
    }

    pub async fn check_upstream_hit(&self, upstreams: &[Url]) -> bool {
        for upstream in upstreams {
            let upstream = upstream
                .join(self.narinfo_path().as_ref())
                .expect("adding <hash>.narinfo should make a valid url");
            trace!("querying {}", upstream);
            let res_status = reqwest::Client::new()
                .head(upstream.as_str())
                .send()
                .await
                .map(|x| x.status());

            if res_status.map(|code| code.is_success()).unwrap_or_default() {
                return true;
            }
        }
        false
    }

    pub fn absolute_path(&self) -> String {
        self.path.to_absolute_path()
    }

    pub fn narinfo_path(&self) -> ObjectPath {
        ObjectPath::parse(format!("{}.narinfo", nixbase32::encode(self.path.digest())))
            .expect("must parse to a valid object_store path")
    }

    pub async fn check_if_already_exists(&self, s3: &AmazonS3) -> bool {
        s3.head(&self.narinfo_path()).await.is_ok()
    }
}
