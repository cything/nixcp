use anyhow::Result;
use futures::StreamExt;
use object_store::{ObjectMeta, ObjectStore};
use std::sync::Arc;
use time::UtcDateTime;

use crate::server::{ServerConfig, db};

/// cutoff -> UNIX timestamp for which anything not accessed since should be purged.
pub async fn run(config: Arc<ServerConfig>, cutoff: i64) -> Result<()> {
    // Interate over narinfos
    let mut list_stream = config.s3.list(None);

    while let Some(meta) = list_stream.next().await.transpose()? {
        if !is_object_narinfo(&meta) {
            continue;
        };

        let nar_hash = meta
            .location
            .as_ref()
            .strip_suffix(".narinfo")
            .expect("suffix mut exist cause we just confirmed it's a narinfo");
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
    let narinfo_data = config
        .s3
        .get(&meta.location)
        .await?;

    Ok(())
}
