use std::collections::HashSet;

use anyhow::{Context, Result};
use aws_sdk_s3 as s3;
use futures::future::join_all;
use nix_compat::nixbase32;
use nix_compat::store_path::StorePath;
use regex::Regex;
use std::path::Path;
use tracing::{debug, trace};
use url::Url;

use crate::store::Store;

#[derive(Debug, Clone)]
pub struct PathInfo {
    pub deriver: Option<StorePath<String>>,
    pub path: StorePath<String>,
    pub signatures: Vec<String>,
    pub references: Vec<StorePath<String>>,
}

impl PathInfo {
    pub async fn from_path(path: &Path, store: &Store) -> Result<Self> {
        debug!("query path info for {:?}", path);
        let canon = path.canonicalize().context("canonicalize path")?;
        let store_path = StorePath::from_absolute_path(canon.into_os_string().as_encoded_bytes())?;
        store.query_path_info(store_path).await
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
        return signers;
    }

    pub async fn check_upstream_hit(&self, upstreams: &[Url]) -> bool {
        for upstream in upstreams {
            let upstream = upstream
                .join(format!("{}.narinfo", self.digest()).as_str())
                .expect("adding <hash>.narinfo should make a valid url");
            debug!("querying {}", upstream);
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

    pub fn digest(&self) -> String {
        nixbase32::encode(self.path.digest())
    }

    pub async fn check_if_already_exists(&self, s3_client: &s3::Client, bucket: String) -> bool {
        s3_client
            .head_object()
            .bucket(bucket)
            .key(format!("{}.narinfo", self.digest()))
            .send()
            .await
            .is_ok()
    }
}
