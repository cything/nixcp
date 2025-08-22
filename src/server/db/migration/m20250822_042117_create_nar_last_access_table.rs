use sea_orm_migration::{prelude::*, schema::*};
use crate::server::db::nar_last_access::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Entity)
                    .if_not_exists()
                    .col(pk_auto(Column::Id))
                    .col(string_uniq(Column::NarHash))
                    .col(big_integer(Column::LastAccessed))
                    .to_owned(),
            )
           .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-nar-hash-timestamp")
                    .table(Entity)
                    .col(Column::NarHash)
                    .to_owned(),
            ).await
    }
}
