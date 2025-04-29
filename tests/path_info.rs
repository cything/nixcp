use nixcp::path_info::PathInfo;
use std::path::PathBuf;
use std::process::Command;

mod common;

const HELLO: &str = "github:nixos/nixpkgs?ref=f771eb401a46846c1aebd20552521b233dd7e18b#hello";
const HELLO_DRV: &str = "iqbwkm8mjjjlmw6x6ry9rhzin2cp9372-hello-2.12.1.drv";

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
    // the path must be in the store
    Command::new("nix")
        .arg("build")
        .arg("--no-link")
        .arg(HELLO)
        .status()
        .unwrap();
    let ctx = common::context();
    let path = PathBuf::from("/nix/store/9bwryidal9q3g91cjm6xschfn4ikd82q-hello-2.12.1");
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
