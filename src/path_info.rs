use std::{collections::HashSet, path::Path};

use anyhow::{Context, Result};
use log::trace;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use url::Url;

// nix path-info --derivation --json
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathInfo {
    deriver: String,
    path: String,
    signatures: Vec<String>,
}
impl PathInfo {
    /// get PathInfo for a package or a store path
    pub async fn from_path(path: &str) -> Result<Self> {
        let path_info = Command::new("nix")
            .arg("path-info")
            .arg("--json")
            .arg(path)
            .output()
            .await
            .context("`nix path-info` failed for {package}")?;

        Ok(serde_json::from_slice(&path_info.stdout)?)
    }

    pub async fn get_closure(&self) -> Result<Vec<Self>> {
        let nix_store_cmd = Command::new("nix-store")
            .arg("--query")
            .arg("--requisites")
            .arg("--include-outputs")
            .arg(&self.deriver)
            .output()
            .await
            .expect("nix-store cmd failed");

        let nix_store_paths = String::from_utf8(nix_store_cmd.stdout)?;
        let nix_store_paths: Vec<&str> = nix_store_paths.lines().collect();
        let mut closure = Vec::with_capacity(nix_store_paths.len());
        for path in nix_store_paths {
            closure.push(Self::from_path(path).await?);
        }
        Ok(closure)
    }

    pub fn get_path(&self) -> &Path {
        &Path::new(&self.path)
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
        return false;
    }

    fn signees(&self) -> Vec<&str> {
        let signees: Vec<_> = self
            .signatures
            .iter()
            .filter_map(|signature| Some(signature.split_once(":")?.0))
            .collect();
        trace!("signees for {}: {:?}", self.path, signees);
        signees
    }

    pub async fn check_upstream_hit(&self, upstreams: &[Url]) -> bool {
        let basename = self.get_path().file_name().unwrap().to_str().unwrap();
        let hash = basename.split_once("-").unwrap().0;

        for upstream in upstreams {
            let upstream = upstream
                .join(format!("{hash}/.narinfo").as_str())
                .expect("adding <hash>.narinfo should make a valid url");
            let res_status = reqwest::Client::new()
                .head(upstream.as_str())
                .send()
                .await
                .map(|x| x.status());

            match &res_status {
                Ok(status) => return status.is_success(),
                Err(_) => return false,
            }
        }
        false
    }
}

impl ToString for PathInfo {
    fn to_string(&self) -> String {
        self.path.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_signees_from_path_info() {
        let path_info = PathInfo {
            deriver: "".to_string(),
            path: "".to_string(),
            signatures: vec![
                "cache.nixos.org-1:sRAGxSFkQ6PGzPGs9caX6y81tqfevIemSSWZjeD7/v1X0J9kEeafaFgz+zBD/0k8imHSWi/leCoIXSCG6/MrCw==".to_string(),
                "nixcache.cy7.sh:hV1VQvztp8UY7hq/G22uzC3vQp4syBtnpJh21I1CRJykqweohb4mdS3enyi+9xXqAUZMfNrZuRFSySqa5WK1Dg==".to_string(),
            ],
        };
        let signees = path_info.signees();
        assert_eq!(signees, vec!["cache.nixos.org-1", "nixcache.cy7.sh"]);
    }

    #[test]
    fn match_upstream_cache_from_signature() {
        let path_info = PathInfo {
            deriver: "".to_string(),
            path: "".to_string(),
            signatures: vec![
                "cache.nixos.org-1:sRAGxSFkQ6PGzPGs9caX6y81tqfevIemSSWZjeD7/v1X0J9kEeafaFgz+zBD/0k8imHSWi/leCoIXSCG6/MrCw==".to_string(),
                "nixcache.cy7.sh:hV1VQvztp8UY7hq/G22uzC3vQp4syBtnpJh21I1CRJykqweohb4mdS3enyi+9xXqAUZMfNrZuRFSySqa5WK1Dg==".to_string(),
                "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs=".to_string(),
            ],
        };
        assert_eq!(
            path_info.check_upstream_signature(&[Url::parse("https://cache.nixos.org").unwrap()]),
            true
        );
        assert_eq!(
            path_info.check_upstream_signature(&[Url::parse("https://nixcache.cy7.sh").unwrap()]),
            true
        );
        assert_eq!(
            path_info.check_upstream_signature(&[
                Url::parse("https://nix-community.cachix.org").unwrap()
            ]),
            true
        );
        assert_eq!(
            path_info
                .check_upstream_signature(&[Url::parse("https://fake-cache.cachix.org").unwrap()]),
            false
        );
    }
}
