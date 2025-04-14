use std::collections::HashSet;

use anyhow::{Context, Error, Result};
use aws_sdk_s3 as s3;
use nix_compat::nixbase32;
use nix_compat::store_path::StorePath;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::{debug, error, trace};
use url::Url;

// nix path-info --derivation --json
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathInfo {
    pub deriver: StorePath<String>,
    pub path: StorePath<String>,
    signatures: Vec<String>,
    pub references: Vec<StorePath<String>>,
}
impl PathInfo {
    /// get PathInfo for a package or a store path
    pub async fn from_path(path: &str) -> Result<Self> {
        debug!("query nix path-info for {path}");
        let nix_cmd = Command::new("nix")
            .arg("path-info")
            .arg("--json")
            .arg(path)
            .output()
            .await
            .context("`nix path-info` failed for {package}")?;

        trace!(
            "nix path-info output: {}",
            String::from_utf8_lossy(&nix_cmd.stdout)
        );

        // nix path-info returns an array with one element
        match serde_json::from_slice::<Vec<_>>(&nix_cmd.stdout)
            .context("parse path info from stdout")
        {
            Ok(path_info) => path_info
                .into_iter()
                .next()
                .ok_or_else(|| Error::msg("nix path-info returned empty")),
            Err(e) => {
                error!(
                    "Failed to parse data from `nix path-info`. The path may not exist on your system."
                );
                Err(e)
            }
        }
    }

    pub async fn get_closure(&self) -> Result<Vec<Self>> {
        debug!("query nix-store for {}", self.absolute_path());
        let nix_store_cmd = Command::new("nix-store")
            .arg("--query")
            .arg("--requisites")
            .arg("--include-outputs")
            .arg(self.absolute_path())
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
        let signees: Vec<_> = self
            .signatures
            .iter()
            .filter_map(|signature| Some(signature.split_once(":")?.0))
            .collect();
        trace!("signees for {}: {:?}", self.path, signees);
        signees
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_signees_from_path_info() {
        let path_info_json = r#"{"deriver":"/nix/store/idy9slp6835nm6x2i41vzm4g1kai1m2p-nixcp-0.1.0.drv.drv","narHash":"sha256-BG5iQEKKOM7d4199942ReE+bZxQDGDuOZqQ5jkTp45o=","narSize":27851376,"path":"/nix/store/giv6gcnv0ymqgi60dx0fsk2l1pxdd1n0-nixcp-0.1.0","references":["/nix/store/954l60hahqvr0hbs7ww6lmgkxvk8akdf-openssl-3.4.1","/nix/store/ik84lbv5jvjm1xxvdl8mhg52ry3xycvm-gcc-14-20241116-lib","/nix/store/rmy663w9p7xb202rcln4jjzmvivznmz8-glibc-2.40-66"],"registrationTime":1744643248,"signatures":["nixcache.cy7.sh:n1lnCoT16xHcuV+tc+/TbZ2m+UKuI15ok+3cg2i5yFHO8+QVUn0x+tOSy6bZ+KxWl4PvmIjUQN1Kus0efn46Cw=="],"valid":true}"#;
        let mut path_info: PathInfo = serde_json::from_str(path_info_json).expect("must serialize");

        path_info.signatures = vec![
            "cache.nixos.org-1:sRAGxSFkQ6PGzPGs9caX6y81tqfevIemSSWZjeD7/v1X0J9kEeafaFgz+zBD/0k8imHSWi/leCoIXSCG6/MrCw==".to_string(),
            "nixcache.cy7.sh:hV1VQvztp8UY7hq/G22uzC3vQp4syBtnpJh21I1CRJykqweohb4mdS3enyi+9xXqAUZMfNrZuRFSySqa5WK1Dg==".to_string(),
        ];
        let signees = path_info.signees();
        assert_eq!(signees, vec!["cache.nixos.org-1", "nixcache.cy7.sh"]);
    }

    #[test]
    fn match_upstream_cache_from_signature() {
        let path_info_json = r#"{"deriver":"/nix/store/idy9slp6835nm6x2i41vzm4g1kai1m2p-nixcp-0.1.0.drv.drv","narHash":"sha256-BG5iQEKKOM7d4199942ReE+bZxQDGDuOZqQ5jkTp45o=","narSize":27851376,"path":"/nix/store/giv6gcnv0ymqgi60dx0fsk2l1pxdd1n0-nixcp-0.1.0","references":["/nix/store/954l60hahqvr0hbs7ww6lmgkxvk8akdf-openssl-3.4.1","/nix/store/ik84lbv5jvjm1xxvdl8mhg52ry3xycvm-gcc-14-20241116-lib","/nix/store/rmy663w9p7xb202rcln4jjzmvivznmz8-glibc-2.40-66"],"registrationTime":1744643248,"signatures":["nixcache.cy7.sh:n1lnCoT16xHcuV+tc+/TbZ2m+UKuI15ok+3cg2i5yFHO8+QVUn0x+tOSy6bZ+KxWl4PvmIjUQN1Kus0efn46Cw=="],"valid":true}"#;
        let mut path_info: PathInfo = serde_json::from_str(path_info_json).expect("must serialize");

        path_info.signatures = vec![
            "cache.nixos.org-1:sRAGxSFkQ6PGzPGs9caX6y81tqfevIemSSWZjeD7/v1X0J9kEeafaFgz+zBD/0k8imHSWi/leCoIXSCG6/MrCw==".to_string(),
            "nixcache.cy7.sh:hV1VQvztp8UY7hq/G22uzC3vQp4syBtnpJh21I1CRJykqweohb4mdS3enyi+9xXqAUZMfNrZuRFSySqa5WK1Dg==".to_string(),
            "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs=".to_string(),
        ];
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
