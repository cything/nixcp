use anyhow::Result;
use async_compression::{Level, tokio::bufread::ZstdEncoder};
use ed25519_dalek;
use nix_compat::{
    narinfo::{self, NarInfo},
    nixbase32,
    store_path::StorePath,
};
use sha2::{Digest, Sha256};
use std::fs;
use tokio::{io::AsyncReadExt, process::Command};

use crate::path_info::PathInfo;

pub struct Uploader {
    signing_key: narinfo::SigningKey<ed25519_dalek::SigningKey>,
    path: PathInfo,
    compression: Option<String>,
}

impl Uploader {
    pub fn new(key_file: &str, path: PathInfo) -> Result<Self> {
        let key = fs::read_to_string(key_file)?;
        let signing_key = narinfo::parse_keypair(key.as_str())?.0;
        Ok(Self {
            signing_key,
            path,
            // TODO: support other algorithms
            compression: Some("zstd".to_string()),
        })
    }

    pub async fn make_nar(&self) -> Result<Vec<u8>> {
        Ok(Command::new("nix")
            .arg("nar")
            .arg("dump-path")
            .arg(self.path.absolute_path())
            .output()
            .await?
            .stdout)
    }

    pub fn narinfo_from_nar(&self, nar: &[u8]) -> Result<NarInfo> {
        let mut hasher = Sha256::new();
        hasher.update(nar);
        let nar_hash: [u8; 32] = hasher.finalize().into();
        let nar_info = NarInfo {
            flags: narinfo::Flags::empty(),
            store_path: self.path.path.as_ref(),
            nar_hash,
            nar_size: nar.len() as u64,
            references: self.path.references.iter().map(StorePath::as_ref).collect(),
            signatures: Vec::new(),
            ca: self.path.ca.clone(),
            system: None,
            deriver: Some(self.path.deriver.as_ref()),
            compression: self.compression.as_ref().map(String::as_str),
            file_hash: None,
            file_size: None,
            url: "",
        };
        Ok(nar_info)
    }

    fn nar_url(&self, compressed_nar_hash: &[u8]) -> String {
        let compressed_nar_hash = nixbase32::encode(compressed_nar_hash);
        format!("nar/{compressed_nar_hash}.nar.zst")
    }

    async fn compress_nar(&self, nar: &[u8]) -> Vec<u8> {
        let mut encoder = ZstdEncoder::with_quality(nar, Level::Default);
        let mut compressed = Vec::with_capacity(nar.len());
        encoder
            .read_to_end(&mut compressed)
            .await
            .expect("should compress just fine");
        compressed
    }
}
