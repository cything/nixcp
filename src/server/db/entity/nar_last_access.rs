use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "nar_last_access")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub nar_hash: String,
    pub last_accessed: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
