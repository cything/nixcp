#![allow(dead_code)]

use std::sync::Arc;

use nixcp::store::Store;

pub const HELLO: &str = "github:nixos/nixpkgs?ref=f771eb401a46846c1aebd20552521b233dd7e18b#hello";
pub const HELLO_DRV: &str = "iqbwkm8mjjjlmw6x6ry9rhzin2cp9372-hello-2.12.1.drv";
pub const HELLO_PATH: &str = "/nix/store/9bwryidal9q3g91cjm6xschfn4ikd82q-hello-2.12.1";

pub struct Context {
    pub store: Arc<Store>,
}

impl Context {
    fn new() -> Self {
        let store = Arc::new(Store::connect().expect("connect to nix store"));
        Self { store }
    }
}

pub fn context() -> Context {
    Context::new()
}
