use anyhow::Result;
use async_compression::{Level, tokio::bufread::ZstdEncoder};
use aws_sdk_s3::{
    self as s3,
    types::{CompletedMultipartUpload, CompletedPart},
};
use futures::future::join_all;
use nix_compat::{
    narinfo::{self, NarInfo, SigningKey},
    nixbase32,
    store_path::StorePath,
};
use sha2::{Digest, Sha256};
use tokio::{io::AsyncReadExt, process::Command};
use tracing::debug;

use crate::path_info::PathInfo;

const MULTIPART_CUTOFF: usize = 1024 * 1024 * 5;

pub struct Uploader<'a> {
    signing_key: &'a SigningKey<ed25519_dalek::SigningKey>,
    path: PathInfo,
    s3_client: &'a s3::Client,
    bucket: String,
}

impl<'a> Uploader<'a> {
    pub fn new(
        signing_key: &'a SigningKey<ed25519_dalek::SigningKey>,
        path: PathInfo,
        s3_client: &'a s3::Client,
        bucket: String,
    ) -> Result<Self> {
        Ok(Self {
            signing_key,
            path,
            s3_client,
            bucket,
        })
    }

    pub async fn upload(&self) -> Result<()> {
        let nar = self.make_nar().await?;
        let mut nar_info = self.narinfo_from_nar(&nar)?;
        let nar = self.compress_nar(&nar).await;

        // update fields that we know after compression
        let mut hasher = Sha256::new();
        hasher.update(&nar);
        let hash: [u8; 32] = hasher.finalize().into();
        let nar_url = self.nar_url(&hash);
        nar_info.file_hash = Some(hash);
        nar_info.file_size = Some(nar.len() as u64);
        nar_info.url = nar_url.as_str();
        debug!("uploading nar with key: {nar_url}");

        if nar.len() < MULTIPART_CUTOFF {
            let put_object = self
                .s3_client
                .put_object()
                .bucket(&self.bucket)
                .key(&nar_url)
                .body(nar.into())
                .send()
                .await?;
            debug!("put object: {:#?}", put_object);
        } else {
            let multipart = self
                .s3_client
                .create_multipart_upload()
                .bucket(&self.bucket)
                .key(&nar_url)
                .send()
                .await?;
            let upload_id = multipart.upload_id().unwrap();

            let mut parts = Vec::with_capacity(nar.len() / MULTIPART_CUTOFF);
            let chunks = nar.chunks(MULTIPART_CUTOFF);
            for (i, chunk) in chunks.enumerate() {
                parts.push(tokio::task::spawn(
                    self.s3_client
                        .upload_part()
                        .bucket(&self.bucket)
                        .key(&nar_url)
                        .upload_id(upload_id)
                        .part_number(i as i32 + 1)
                        .body(chunk.to_vec().into())
                        .send(),
                ));
            }

            let completed_parts = join_all(parts)
                .await
                .into_iter()
                .flatten()
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .enumerate()
                .map(|(i, part)| {
                    CompletedPart::builder()
                        .set_e_tag(part.e_tag().map(ToString::to_string))
                        .set_part_number(Some(i as i32 + 1))
                        .set_checksum_sha256(part.checksum_sha256().map(ToString::to_string))
                        .build()
                })
                .collect::<Vec<_>>();

            let completed_mp_upload = CompletedMultipartUpload::builder()
                .set_parts(Some(completed_parts))
                .build();

            let complete_mp_upload = self
                .s3_client
                .complete_multipart_upload()
                .bucket(&self.bucket)
                .key(&nar_url)
                .upload_id(upload_id)
                .multipart_upload(completed_mp_upload)
                .send()
                .await?;

            debug!("complete multipart upload: {:#?}", complete_mp_upload);
        }

        let narinfo_url = format!("{}.narinfo", self.path.digest());
        debug!("uploading narinfo with key {narinfo_url}");
        self.s3_client
            .put_object()
            .bucket(&self.bucket)
            .key(narinfo_url)
            .body(nar_info.to_string().as_bytes().to_vec().into())
            .send()
            .await?;

        Ok(())
    }

    async fn make_nar(&self) -> Result<Vec<u8>> {
        Ok(Command::new("nix")
            .arg("nar")
            .arg("dump-path")
            .arg(self.path.absolute_path())
            .output()
            .await?
            .stdout)
    }

    fn narinfo_from_nar(&self, nar: &[u8]) -> Result<NarInfo> {
        let mut hasher = Sha256::new();
        hasher.update(nar);
        let nar_hash: [u8; 32] = hasher.finalize().into();
        let mut nar_info = NarInfo {
            flags: narinfo::Flags::empty(),
            store_path: self.path.path.as_ref(),
            nar_hash,
            nar_size: nar.len() as u64,
            references: self.path.references.iter().map(StorePath::as_ref).collect(),
            signatures: Vec::new(),
            ca: None,
            system: None,
            deriver: None,
            compression: Some("zstd"),
            file_hash: None,
            file_size: None,
            url: "",
        };
        // signature consists of: store_path, nar_hash, nar_size, and references
        nar_info.add_signature(self.signing_key);
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
