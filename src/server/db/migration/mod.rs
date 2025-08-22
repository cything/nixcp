use sea_orm_migration::*;

mod m20250822_042117_create_nar_last_access_table;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250822_042117_create_nar_last_access_table::Migration)
        ]
    }
}
