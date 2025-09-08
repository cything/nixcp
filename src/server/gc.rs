use std::sync::Arc;

use anyhow::{Context, Result};
use futures::StreamExt;
use nix_compat::narinfo::NarInfo;
use object_store::{ObjectMeta, ObjectStore};
use tracing::debug;

use crate::server::{ServerConfig, db};

/// cutoff -> UNIX timestamp for which anything not accessed since should be purged.
pub async fn run(config: Arc<ServerConfig>, cutoff: i64) -> Result<()> {
    // Interate over narinfos
    let mut list_stream = config.s3.list(None);

    while let Some(meta) = list_stream.next().await.transpose()? {
        if !is_object_narinfo(&meta) {
            continue;
        };
        debug!("checking {}", &meta.location);

        let nar_hash = meta
            .location
            .as_ref()
            .strip_suffix(".narinfo")
            .expect("suffix must exist cause we just confirmed it's a narinfo");
        let last_access = db::get_narinfo_last_access(&config.db_conn, nar_hash)
            .await
            .unwrap_or_else(|_| meta.last_modified.timestamp());

        if last_access < cutoff {
            purge_narinfo(config.clone(), &meta).await?;
        }
    }
    Ok(())
}

fn is_object_narinfo(meta: &ObjectMeta) -> bool {
    if meta.location.as_ref().ends_with(".narinfo") {
        return true;
    }
    false
}

async fn purge_narinfo(config: Arc<ServerConfig>, meta: &ObjectMeta) -> Result<()> {
    // get narinfo
    let narinfo_data = config.s3.get(&meta.location).await?.bytes().await?;
    let narinfo_str = String::from_utf8(narinfo_data.to_vec())
        .context("parse narinfo fetched from s3 to string")?;
    let narinfo =
        NarInfo::parse(&narinfo_str).context("parse narinfo fetched from s3 to NarInfo")?;

    // delete the narinfo first so nobody tries to get the nar it points to
    config
        .s3
        .delete(&meta.location)
        .await
        .context("delete narinfo from s3 bucket")?;

    // delete nar at the location narinfo pointed to
    config
        .s3
        .delete(&narinfo.url.into())
        .await
        .context("delete nar from s3 bucket")?;

    Ok(())
}
