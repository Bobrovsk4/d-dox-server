use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(FileVersions::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FileVersions::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(FileVersions::FileId).integer().not_null())
                    .col(ColumnDef::new(FileVersions::Version).integer().not_null())
                    .col(ColumnDef::new(FileVersions::Size).big_integer().not_null())
                    .col(ColumnDef::new(FileVersions::AuthorId).integer().not_null())
                    .col(
                        ColumnDef::new(FileVersions::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-file_versions-file_id")
                            .from(FileVersions::Table, FileVersions::FileId)
                            .to(Files::Table, Files::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-file_versions-author_id")
                            .from(FileVersions::Table, FileVersions::AuthorId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(FileVersions::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum FileVersions {
    Table,
    Id,
    FileId,
    Version,
    Size,
    AuthorId,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Files {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
