use nixcp::path_info::PathInfo;
use std::path::PathBuf;

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
async fn closure() {
    let ctx = common::context();
    let path = PathBuf::from(HELLO);
    let path_info = PathInfo::from_derivation(&path, &ctx.store)
        .await
        .expect("get pathinfo from package");
    let closure = path_info.get_closure(&ctx.store).await.unwrap();
    assert_eq!(closure.len(), 466);
}
