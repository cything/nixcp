use anyhow::{Context, Result};
use entity::nar_last_access;
use sea_orm::*;
use sea_orm_migration::MigratorTrait;
use time::UtcDateTime;
use tracing::debug;

mod entity;
mod migration;

pub async fn run_migrations(db: &DatabaseConnection) -> Result<()> {
    eprintln!("Running migrations...");
    migration::Migrator::up(db, None).await?;

    Ok(())
}

pub async fn get_narinfo_last_access(db: &DatabaseConnection, hash: &str) -> Result<i64> {
    let narinfo = nar_last_access::Entity::find()
        .filter(nar_last_access::Column::NarHash.eq(hash))
        .one(db)
        .await?
        .context(format!("no entry in db for nar {hash}"))?;

    Ok(narinfo.last_accessed)
}

pub async fn update_narinfo_last_access(db: &DatabaseConnection, hash: &str) -> Result<()> {
    let narinfo = nar_last_access::Entity::find()
        .filter(nar_last_access::Column::NarHash.eq(hash))
        .one(db)
        .await?;

    let now_timestamp = UtcDateTime::now().unix_timestamp();
    if let Some(narinfo) = narinfo {
        let mut narinfo: nar_last_access::ActiveModel = narinfo.into();
        narinfo.last_accessed = Set(now_timestamp);
        narinfo.update(db).await?;
        debug!("update access time to {} for {}.narinfo", now_timestamp, hash);
    } else {
        let narinfo = nar_last_access::ActiveModel {
            nar_hash: Set(hash.to_string()),
            last_accessed: Set(now_timestamp),
            ..Default::default()
        };
        narinfo.insert(db).await?;
        debug!("new access time {} for {}.narinfo", now_timestamp, hash);
    }
    Ok(())
}
