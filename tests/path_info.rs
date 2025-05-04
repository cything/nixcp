use nixcp::path_info::PathInfo;
use std::{collections::HashSet, path::PathBuf, process::Command};

use tempfile::TempDir;

use crate::common::{HELLO, HELLO_DRV, HELLO_PATH, NIXCP_DRV, NIXCP_PKG};

mod common;

#[tokio::test]
async fn path_info_from_package() {
    let ctx = common::context();
    let path = PathBuf::from(HELLO);
    let path_info = PathInfo::from_derivation(&path, &ctx.store)
        .await
        .expect("get pathinfo from package");
    assert_eq!(path_info.path.to_absolute_path(), HELLO_DRV);
}

#[tokio::test]
async fn path_info_from_path() {
    let ctx = common::context();
    let path = PathBuf::from(HELLO_PATH);
    let path_info = PathInfo::from_derivation(&path, &ctx.store)
        .await
        .expect("get pathinfo from package");
    assert_eq!(path_info.path.to_absolute_path(), HELLO_DRV);
}

#[tokio::test]
async fn path_info_symlink() {
    let ctx = common::context();

    let temp_path = TempDir::new().unwrap();
    let link_path = temp_path.path().join("result");

    // symlink at ./result (like `nix build`)
    std::os::unix::fs::symlink(HELLO_PATH, &link_path).unwrap();

    // should resolve symlink
    let path_info = PathInfo::from_derivation(&link_path, &ctx.store)
        .await
        .expect("get pathinfo from package");
    assert_eq!(path_info.path.to_absolute_path(), HELLO_DRV);
}

#[tokio::test]
async fn closure_includes_nix_store_requisites() {
    let ctx = common::context();
    let path = PathBuf::from(HELLO);
    let path_info = PathInfo::from_derivation(&path, &ctx.store)
        .await
        .expect("get pathinfo from package");

    // get what we think is the closure
    let mut closure: HashSet<String> = path_info
        .get_closure(&ctx.store)
        .await
        .unwrap()
        .iter()
        .map(|x| x.path.to_absolute_path())
        .collect();

    // for a somewhat more complicated case
    common::ensure_exists(NIXCP_PKG);
    let path = PathBuf::from(NIXCP_PKG);
    let path_info = PathInfo::from_derivation(&path, &ctx.store)
        .await
        .expect("get pathinfo from package");
    closure.extend(
        path_info
            .get_closure(&ctx.store)
            .await
            .unwrap()
            .iter()
            .map(|x| x.path.to_absolute_path()),
    );

    // get output of `nix-store --query --requisites --include-outputs`
    let nix_store_out = Command::new("nix-store")
        .arg("--query")
        .arg("--requisites")
        .arg("--include-outputs")
        .arg(HELLO_DRV)
        .arg(NIXCP_DRV)
        .output()
        .unwrap()
        .stdout;
    assert!(!nix_store_out.is_empty());
    let ref_closure = String::from_utf8_lossy(&nix_store_out);
    let ref_closure = ref_closure.split_whitespace();

    // check that we didn't miss anything nix-store would catch
    for path in ref_closure {
        assert!(closure.contains(path));
    }
}
