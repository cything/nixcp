#![allow(dead_code)]

use std::process::Command;
use std::sync::Arc;

use nixcp::store::Store;

pub const HELLO: &str = "github:nixos/nixpkgs?ref=f771eb401a46846c1aebd20552521b233dd7e18b#hello";
pub const HELLO_DRV: &str = "/nix/store/iqbwkm8mjjjlmw6x6ry9rhzin2cp9372-hello-2.12.1.drv";
pub const HELLO_PATH: &str = "/nix/store/9bwryidal9q3g91cjm6xschfn4ikd82q-hello-2.12.1";
pub const NIXCP_PKG: &str = "github:cything/nixcp?ref=6cfe67af0e8da502702b31f34a941753e64d9561";
pub const NIXCP_DRV: &str = "/nix/store/ldjvf9qjp980dyvka2hj99q4c0w6901x-nixcp-0.1.0.drv";

pub struct Context {
    pub store: Arc<Store>,
}

impl Context {
    fn new() -> Self {
        // hello must be in the store
        ensure_exists(HELLO);
        let store = Arc::new(Store::connect().expect("connect to nix store"));
        Self { store }
    }
}

pub fn context() -> Context {
    Context::new()
}

pub fn ensure_exists(pkg: &str) {
    Command::new("nix")
        .arg("build")
        .arg("--no-link")
        .arg(pkg)
        .status()
        .unwrap();
}
