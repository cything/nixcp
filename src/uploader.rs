use anyhow::Result;
use bytes::BytesMut;
use nix_compat::{narinfo::SigningKey, nixbase32};
use object_store::{ObjectStore, aws::AmazonS3, buffered::BufWriter, path::Path};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, trace};
use ulid::Ulid;

use crate::{make_nar::MakeNar, path_info::PathInfo};

const CHUNK_SIZE: usize = 1024 * 1024 * 5;

pub struct Uploader<'a> {
    signing_key: &'a SigningKey<ed25519_dalek::SigningKey>,
    path: PathInfo,
}

impl<'a> Uploader<'a> {
    pub fn new(
        signing_key: &'a SigningKey<ed25519_dalek::SigningKey>,
        path: PathInfo,
    ) -> Result<Self> {
        Ok(Self { signing_key, path })
    }

    pub async fn upload(&self, s3: Arc<AmazonS3>) -> Result<()> {
        let mut nar = MakeNar::new(&self.path)?;
        nar.make().await?;

        // we don't know what the hash of the compressed file will be so upload to a
        // temp location for now
        let temp_path = Path::parse(Ulid::new().to_string())?;
        let mut s3_writer = BufWriter::new(s3.clone(), temp_path.clone());

        // compress and upload nar
        let mut file_reader = nar.compress_and_hash().await?;
        let mut buf = BytesMut::with_capacity(CHUNK_SIZE);
        debug!("uploading to temp path: {}", temp_path);
        while let n = file_reader.read_buf(&mut buf).await?
            && n != 0
        {
            s3_writer.write_all_buf(&mut buf).await?;
        }
        drop(file_reader);

        let mut nar_info = nar.get_narinfo()?;
        nar_info.add_signature(self.signing_key);
        trace!("narinfo: {:#}", nar_info);

        // now that we can calculate the file_hash move the nar to where it should be
        let real_path = nar_url(
            &nar_info
                .file_hash
                .expect("file hash must be known at this point"),
        );
        debug!("moving {} to {}", temp_path, real_path);
        // this is implemented as a copy-and-delete
        s3.rename(&temp_path, &real_path).await?;

        // upload narinfo
        let narinfo_path = self.path.narinfo_path();
        debug!("uploading narinfo: {}", narinfo_path);
        s3.put(&narinfo_path, nar_info.to_string().into()).await?;

        Ok(())
    }
}

/// calculate url where the compressed nar should be uploaded
fn nar_url(file_hash: &[u8]) -> Path {
    let compressed_nar_hash = nixbase32::encode(file_hash);
    Path::parse(format!("nar/{compressed_nar_hash}.nar.zst"))
        .expect("should parse to a valid object_store::path::Path")
}
