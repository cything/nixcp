use std::collections::HashSet;

use anyhow::{Context, Error, Result};
use aws_sdk_s3 as s3;
use nix_compat::nixbase32;
use nix_compat::store_path::StorePath;
use regex::Regex;
use serde::Deserialize;
use tokio::process::Command;
use tracing::{debug, error, trace};
use url::Url;

// nix path-info --derivation --json
#[derive(Debug, Clone, Deserialize)]
pub struct PathInfo {
    pub deriver: Option<StorePath<String>>,
    pub path: StorePath<String>,
    signatures: Option<Vec<String>>,
    pub references: Vec<StorePath<String>>,
}

impl PathInfo {
    // get PathInfo for a package or a store path
    // we deserialize this as an array of `PathInfo` below
    pub async fn from_path(path: &str) -> Result<Self> {
        debug!("query nix path-info for {path}");
        // use lix cause nix would return a json map instead of an array
        // json output is not stable and could break in future
        // TODO figure out a better way
        let nix_cmd = Command::new("nix")
            .arg("run")
            .arg("--experimental-features")
            .arg("nix-command flakes")
            .arg("github:nixos/nixpkgs/nixos-unstable#lix")
            .arg("--")
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
        if let Some(signatures) = self.signatures.as_ref() {
            let signees: Vec<_> = signatures
                .iter()
                .filter_map(|signature| Some(signature.split_once(":")?.0))
                .collect();
            trace!("signees for {}: {:?}", self.path, signees);
            return signees;
        }
        Vec::new()
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

        path_info.signatures = Some(vec![
            "cache.nixos.org-1:sRAGxSFkQ6PGzPGs9caX6y81tqfevIemSSWZjeD7/v1X0J9kEeafaFgz+zBD/0k8imHSWi/leCoIXSCG6/MrCw==".to_string(),
            "nixcache.cy7.sh:hV1VQvztp8UY7hq/G22uzC3vQp4syBtnpJh21I1CRJykqweohb4mdS3enyi+9xXqAUZMfNrZuRFSySqa5WK1Dg==".to_string(),
        ]);
        let signees = path_info.signees();
        assert_eq!(signees, vec!["cache.nixos.org-1", "nixcache.cy7.sh"]);
    }

    #[test]
    fn match_upstream_cache_from_signature() {
        let path_info_json = r#"{"deriver":"/nix/store/idy9slp6835nm6x2i41vzm4g1kai1m2p-nixcp-0.1.0.drv.drv","narHash":"sha256-BG5iQEKKOM7d4199942ReE+bZxQDGDuOZqQ5jkTp45o=","narSize":27851376,"path":"/nix/store/giv6gcnv0ymqgi60dx0fsk2l1pxdd1n0-nixcp-0.1.0","references":["/nix/store/954l60hahqvr0hbs7ww6lmgkxvk8akdf-openssl-3.4.1","/nix/store/ik84lbv5jvjm1xxvdl8mhg52ry3xycvm-gcc-14-20241116-lib","/nix/store/rmy663w9p7xb202rcln4jjzmvivznmz8-glibc-2.40-66"],"registrationTime":1744643248,"signatures":["nixcache.cy7.sh:n1lnCoT16xHcuV+tc+/TbZ2m+UKuI15ok+3cg2i5yFHO8+QVUn0x+tOSy6bZ+KxWl4PvmIjUQN1Kus0efn46Cw=="],"valid":true}"#;
        let mut path_info: PathInfo = serde_json::from_str(path_info_json).expect("must serialize");

        path_info.signatures = Some(vec![
            "cache.nixos.org-1:sRAGxSFkQ6PGzPGs9caX6y81tqfevIemSSWZjeD7/v1X0J9kEeafaFgz+zBD/0k8imHSWi/leCoIXSCG6/MrCw==".to_string(),
            "nixcache.cy7.sh:hV1VQvztp8UY7hq/G22uzC3vQp4syBtnpJh21I1CRJykqweohb4mdS3enyi+9xXqAUZMfNrZuRFSySqa5WK1Dg==".to_string(),
            "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs=".to_string(),
        ]);
        assert!(
            path_info.check_upstream_signature(&[Url::parse("https://cache.nixos.org").unwrap()])
        );
        assert!(
            path_info.check_upstream_signature(&[Url::parse("https://nixcache.cy7.sh").unwrap()])
        );
        assert!(
            path_info.check_upstream_signature(&[
                Url::parse("https://nix-community.cachix.org").unwrap()
            ])
        );
        assert!(
            !path_info
                .check_upstream_signature(&[Url::parse("https://fake-cache.cachix.org").unwrap()]),
        );
    }

    #[test]
    fn path_info_without_signature() {
        let path_info_json = r#"{"ca":"fixed:r:sha256:1q10p04pgx9sk6xbvrkn4nvh0ys2lzplgcni5368f4z3cr8ikbmz","narHash":"sha256-v64ZUWbjE4fMKNGyR++nQnsAtyV25r26mTr1dwm4IOA=","narSize":5520,"path":"/nix/store/gj6hz9mj23v01yvq1nn5f655jrcky1qq-nixos-option.nix","references":[],"registrationTime":1744740942,"valid":true}"#;
        let path_info: PathInfo = serde_json::from_str(path_info_json).expect("must serialize");

        assert!(
            !path_info.check_upstream_signature(&[Url::parse("https://cache.nixos.org").unwrap()])
        );
    }

    /*
    #[test]
    fn path_info_deserialize_nix_map() {
        let path_info_json = r#"{"/nix/store/8vm1jxsc0jphd65vb7r6g5ysgqw0yh9f-home-manager-generation":{"ca":null,"deriver":"/nix/store/h8z25s6arcrns5nmrq1yhgbamywjivpn-home-manager-generation.drv","narHash":"sha256-o4qwqyJ5UVm9cyC/nBNcNYVnIM14Pewgw7fou+wUVSY=","narSize":13608,"references":["/nix/store/40yifhx34v4g4llrdn3v2ag8w02j10fv-gnugrep-3.11","/nix/store/4d0ix5djms3n2jnjdc58l916cwack1rp-empty-directory","/nix/store/56zmgla8443qfrkrh2ch0vz0mh8ywrw1-home-manager-files","/nix/store/58br4vk3q5akf4g8lx0pqzfhn47k3j8d-bash-5.2p37","/nix/store/80l1sb3vcmrkcdd7ihlizkcnv19rq9fj-ncurses-6.5","/nix/store/8vm1jxsc0jphd65vb7r6g5ysgqw0yh9f-home-manager-generation","/nix/store/92as847i10kl6s19fi910ddyk9l83835-check-link-targets.sh","/nix/store/9c90iz95yynyh3vsc67zndch6j01vgz3-home-manager-path","/nix/store/b2cfj7yk3wfg1jdwjzim7306hvsc5gnl-systemd-257.3","/nix/store/bm5fi6wj0w4r2wjll2448k307bzfcjwx-cleanup","/nix/store/c244fsb3a7i5837lzn94m4bmav9i5p9b-link","/nix/store/cvlbhhrvzfkjl2hrrzhq3vr5gzan1r60-bash-interactive-5.2p37","/nix/store/gpxsdrrd4x93fs75395vr2dfys1ki9mq-jq-1.7.1-bin","/nix/store/jlf743lqxbvad6dbgndsgqfg20m2np5i-sd-switch-0.5.3","/nix/store/mhmgm739aagj4x7hr6ag2wjmxhmpy8mf-gettext-0.22.5","/nix/store/w9db12j05yv5hl31s6jndd9cfm1g1gw4-hm-modules-messages","/nix/store/wj1c3gsiajabnq50ifxqnlv60i5rhqj7-diffutils-3.10","/nix/store/xhql0ilzbiqwnmz4z8y0phk611wynxf2-gnused-4.9","/nix/store/xq5f95pp297afc2xjgrmhmf9w631qp7m-findutils-4.10.0","/nix/store/yh6qg1nsi5h2xblcr67030pz58fsaxx3-coreutils-9.6","/nix/store/zhrjg6wxrxmdlpn6iapzpp2z2vylpvw5-home-manager.sh"],"registrationTime":1744742989,"signatures":["nixcache.cy7.sh:Vq4X95kSzum7BwrBhjmmM2yVipfBI3AE3jgZ3b3RoYrP4/ghotbDdlwCvwK3qx4BQdEOLSgrC1tDwiMNb6oRBw=="],"ultimate":false}}"#;
        serde_json::from_str::<HashMap<String, PathInfo>>(path_info_json).expect("must serialize");

        let path_info_json = r#"{"/nix/store/3a2ahdaprw6df0lml1pj9jhbi038dsjh-nixos-system-chunk-25.05.20250412.2631b0b":{"ca":null,"deriver":"/nix/store/12ssi931481jlkizgfk1c1jnawvwjbhh-nixos-system-chunk-25.05.20250412.2631b0b.drv","narHash":"sha256-CHhBIzMD4v/FKqKgGroq0UC1k3GrK5lcNwQPMpv2xLc=","narSize":20704,"references":["/nix/store/0yjiyixxsr137iw93hnaacdsssy1li9h-switch-to-configuration-0.1.0","/nix/store/14rby7cpwrzjsjym44cl5h6nj6qpn1gs-etc","/nix/store/3a2ahdaprw6df0lml1pj9jhbi038dsjh-nixos-system-chunk-25.05.20250412.2631b0b","/nix/store/3wjljpj30fvv2cdb60apr4126pa5bm87-shadow-4.17.2","/nix/store/40yifhx34v4g4llrdn3v2ag8w02j10fv-gnugrep-3.11","/nix/store/58br4vk3q5akf4g8lx0pqzfhn47k3j8d-bash-5.2p37","/nix/store/5dyh8l59kfvf89zjkbmjfnx7fix93n4f-net-tools-2.10","/nix/store/aq9wdsz12bg9252790l9awiry2bml4ls-sops-install-secrets-0.0.1","/nix/store/b00kq6fjhgisdrykg621vml8505nnmb3-users-groups.json","/nix/store/b2cfj7yk3wfg1jdwjzim7306hvsc5gnl-systemd-257.3","/nix/store/bfr68wi6k8icb3j9fy3fzchva56djfhd-mounts.sh","/nix/store/cjnihsds5hhnji9r85hglph07q9y9hgc-system-path","/nix/store/cvlbhhrvzfkjl2hrrzhq3vr5gzan1r60-bash-interactive-5.2p37","/nix/store/f9jll96j74f5ykvs062718b98lfjbn9g-util-linux-2.40.4-bin","/nix/store/h7zih134d3n5yk8pnhv1fa38n6qkyrn2-pre-switch-checks","/nix/store/idn5n51246piyxcr3v6gxnj5a5l9mzpn-linux-6.14.2","/nix/store/ipn5793y61x2904xqnkgbjnp91svjjzx-perl-5.40.0-env","/nix/store/j1rikvl25pz0b5ham1ijq0nbg1q2fqfy-initrd-linux-6.14.2","/nix/store/jgawnqyh6piwcl79gxpmq5czx9rfr9xh-glibc-locales-2.40-66","/nix/store/jqgmcv8j4gj59218hcbiyn8z951rycdj-install-grub.sh","/nix/store/kpmybhxy3gz6k1znbdirwsp3c6wvsgg9-manifest.json","/nix/store/lgainx4gl6q7mhiwmls81d3n51p5jz7z-linux-6.14.2-modules","/nix/store/mhxn5kwnri3z9hdzi3x0980id65p0icn-lib.sh","/nix/store/n8n0faszqlnf3mdg0fj6abnknrhjsw5j-perl-5.40.0-env","/nix/store/nq61v7a601gjndijq5nndprkzpwz4q9g-glibc-2.40-66-bin","/nix/store/nx27idxpvi3fk3p7admvhipny73nr25n-kmod-31","/nix/store/pggww1d2pg24fcg5v36xn63n53vanyyi-gnupg-2.4.7","/nix/store/rg5rf512szdxmnj9qal3wfdnpfsx38qi-setup-etc.pl","/nix/store/vvlfaafnz3pdhw7lx5kc5gb9pl4zhz5l-local-cmds","/nix/store/w142vx7ij1fz6qwhp5dprkf59cizvv1v-update-users-groups.pl","/nix/store/xq5f95pp297afc2xjgrmhmf9w631qp7m-findutils-4.10.0","/nix/store/yh6qg1nsi5h2xblcr67030pz58fsaxx3-coreutils-9.6","/nix/store/zlsmh0ccgvncg30qb4y0mp5pahnk1wnw-append-initrd-secrets","/nix/store/zs07icpv5ykf8m36xcv717hh26bp09fa-firmware","/nix/store/zy2n4id5gcxcbx2x8jbblkmcpdlpsypk-getent-glibc-2.40-66"],"registrationTime":1744743136,"signatures":["nixcache.cy7.sh:dZ1XiKQNe0fRX48gBj03PIABYJGV6BPwb72YpMqEBONZMF+JrkVKhRCF0ur/4Bf5prHxg6Qfg1ytP/4csRC9DQ=="],"ultimate":false}}"#;
        serde_json::from_str::<HashMap<String, PathInfo>>(path_info_json).expect("must serialize");
    }
    */
}
