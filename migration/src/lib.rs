#![allow(elided_lifetimes_in_paths)]
#![allow(clippy::wildcard_imports)]
pub use sea_orm_migration::prelude::*;

mod m20250101_000001_create_roles;
mod m20250101_000002_create_users;
mod m20250101_000003_create_files;
mod m20250101_000004_seed_default_data;
mod m20250101_000005_add_updated_at_to_files;
mod m20250101_000006_add_version_to_files;
mod m20250101_000007_create_file_versions;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250101_000001_create_roles::Migration),
            Box::new(m20250101_000002_create_users::Migration),
            Box::new(m20250101_000003_create_files::Migration),
            Box::new(m20250101_000004_seed_default_data::Migration),
            Box::new(m20250101_000005_add_updated_at_to_files::Migration),
            Box::new(m20250101_000006_add_version_to_files::Migration),
            Box::new(m20250101_000007_create_file_versions::Migration),
            // inject-above (do not remove this comment)
        ]
    }
}
