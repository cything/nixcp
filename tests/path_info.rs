use nixcp::path_info::PathInfo;
use std::path::PathBuf;

use tempfile::TempDir;

use crate::common::{HELLO, HELLO_DRV, HELLO_PATH};

mod common;

#[tokio::test]
async fn path_info_from_package() {
    let ctx = common::context();
    let path = PathBuf::from(HELLO);
    let path_info = PathInfo::from_derivation(&path, &ctx.store)
        .await
        .expect("get pathinfo from package");
    assert_eq!(path_info.path.to_string(), HELLO_DRV);
}

#[tokio::test]
async fn path_info_from_path() {
    let ctx = common::context();
    let path = PathBuf::from(HELLO_PATH);
    let path_info = PathInfo::from_derivation(&path, &ctx.store)
        .await
        .expect("get pathinfo from package");
    assert_eq!(path_info.path.to_string(), HELLO_DRV);
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
    assert_eq!(path_info.path.to_string(), HELLO_DRV);
}

/*
#[tokio::test]
async fn closure() {
    let ctx = common::context();
    let path = PathBuf::from(HELLO);
    let path_info = PathInfo::from_derivation(&path, &ctx.store)
        .await
        .expect("get pathinfo from package");
    let closure = path_info.get_closure(&ctx.store).await.unwrap();
    assert_eq!(closure.len(), 472);
}
*/
