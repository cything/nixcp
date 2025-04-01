#![feature(string_from_utf8_lossy_owned)]

use std::{env, path::Path};
use std::process::Command;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json;
use log::debug;

const UPSTREAM_CACHES: &'static [&'static str] = &[
    "https://cache.nixos.org",
    "https://nix-community.cachix.org",
    "https://nixcache.cy7.sh",
];

// nix path-info --derivation --json
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathInfo {
    ca: String,
    nar_hash: String,
    nar_size: u32,
    path: String,
    references: Vec<String>,
    registration_time: u32,
    valid: bool,
}

impl PathInfo {
    // find store paths related to derivation
    fn get_store_paths(&self) -> Vec<String> {
        let mut store_paths: Vec<String> = Vec::new();
        let nix_store_cmd = Command::new("nix-store")
            .arg("--query")
            .arg("--requisites")
            .arg("--include-outputs")
            .arg(&self.path)
            .output()
            .expect("nix-store cmd failed");

        let nix_store_out = String::from_utf8(nix_store_cmd.stdout).unwrap();
        for store_path in nix_store_out.split_whitespace().map(ToString::to_string) {
            store_paths.push(store_path);
        }
        store_paths
    }
}

fn main() {
    env_logger::init();
    let args: Vec<String> = env::args().collect();
    let package = &args[1];
    println!("package: {}", package);

    // find derivations related to package
    let path_infos = Command::new("nix")
        .arg("path-info")
        .arg("--derivation")
        .arg("--json")
        .arg(package)
        .output()
        .expect("path-info failed");

    let path_infos: Vec<PathInfo> = serde_json::from_slice(&path_infos.stdout).unwrap();
    debug!("PathInfo's from nix path-info: {:#?}", path_infos);


    // filter out store paths that exist in upstream caches
    let store_paths = path_infos[0].get_store_paths();
    for store_path in store_paths {
        let basename = Path::new(&store_path).file_name().unwrap().to_str().unwrap().to_string();
        let hash = basename.split("-").nth(0).unwrap();
        println!("hash: {}", hash);
    }
}
