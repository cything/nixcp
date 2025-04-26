use anyhow::{Context, Result};
use async_compression::{Level, tokio::bufread::ZstdEncoder};
use nix_compat::{
    narinfo::{self, NarInfo},
    store_path::StorePath,
};
use sha2::{Digest, Sha256};
use std::mem::take;
use tempfile::{NamedTempFile, TempPath};
use tokio::{
    fs::File,
    io::{AsyncRead, BufReader},
    process::Command,
};
use tokio_util::io::InspectReader;
use tracing::debug;

use crate::path_info::PathInfo;

pub struct MakeNar<'a> {
    path_info: &'a PathInfo,
    nar_file: TempPath,
    nar_hasher: Sha256,
    /// hash of compressed nar file
    file_hasher: Sha256,
    nar_size: u64,
    file_size: u64,
}

impl<'a> MakeNar<'a> {
    pub fn new(path_info: &'a PathInfo) -> Result<Self> {
        Ok(Self {
            path_info,
            nar_file: NamedTempFile::new()
                .context("create tempfile for nar")?
                .into_temp_path(),
            nar_hasher: Sha256::new(),
            file_hasher: Sha256::new(),
            nar_size: 0,
            file_size: 0,
        })
    }

    pub async fn make(&self) -> Result<()> {
        let path = self.path_info.absolute_path();
        let out = std::fs::File::options()
            .write(true)
            .truncate(true)
            .open(&self.nar_file)?;

        debug!("dumping nar for: {}", path);
        Ok(Command::new("nix")
            .arg("nar")
            .arg("pack")
            .arg(self.path_info.absolute_path())
            .kill_on_drop(true)
            .stdout(out)
            .spawn()?
            .wait()
            .await?
            .exit_ok()?)
    }

    /// Returns a compressed nar reader which can be uploaded. File hash will be available when
    /// everything is read
    pub async fn compress_and_hash(&mut self) -> Result<impl AsyncRead> {
        let nar_file = File::open(&self.nar_file).await?;
        // reader that hashes as nar is read
        let nar_reader = InspectReader::new(nar_file, |x| {
            self.nar_size += x.len() as u64;
            self.nar_hasher.update(x);
        });

        let encoder = ZstdEncoder::with_quality(BufReader::new(nar_reader), Level::Default);
        // reader that updates file_hash as the compressed nar is read
        Ok(InspectReader::new(encoder, |x| {
            self.file_size += x.len() as u64;
            self.file_hasher.update(x);
        }))
    }

    /// Returns *unsigned* narinfo. `url` must be updated before uploading
    pub fn get_narinfo(&mut self) -> Result<NarInfo> {
        let file_hash = take(&mut self.file_hasher).finalize().into();
        let nar_hash = take(&mut self.nar_hasher).finalize().into();

        Ok(NarInfo {
            flags: narinfo::Flags::empty(),
            store_path: self.path_info.path.as_ref(),
            nar_hash,
            nar_size: self.nar_size,
            references: self
                .path_info
                .references
                .iter()
                .map(StorePath::as_ref)
                .collect(),
            signatures: Vec::new(),
            ca: None,
            system: None,
            deriver: None,
            compression: Some("zstd"),
            file_hash: Some(file_hash),
            file_size: Some(self.file_size),
            url: "",
        })
    }
}
