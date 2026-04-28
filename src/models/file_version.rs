use chrono::Utc;
use loco_rs::prelude::*;
use sea_orm::{ActiveValue::NotSet, QueryOrder, entity::prelude::*};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "file_versions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub file_id: i32,
    pub version: i32,
    pub size: i64,
    pub author_id: i32,
    #[sea_orm(column_type = "Timestamp")]
    pub created_at: sea_orm::prelude::DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::file::Entity",
        from = "Column::FileId",
        to = "super::file::Column::Id"
    )]
    File,
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::AuthorId",
        to = "super::user::Column::Id"
    )]
    Author,
}

impl Related<super::file::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::File.def()
    }
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Author.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

pub async fn create(
    db: &DatabaseConnection,
    file_id: i32,
    version: i32,
    size: i64,
    author_id: i32,
) -> Result<Model, DbErr> {
    let now = Utc::now().naive_utc();
    let res = Entity::insert(ActiveModel {
        id: NotSet,
        file_id: Set(file_id),
        version: Set(version),
        size: Set(size),
        author_id: Set(author_id),
        created_at: Set(now),
    })
    .exec(db)
    .await?;

    Entity::find_by_id(res.last_insert_id)
        .one(db)
        .await?
        .ok_or(DbErr::RecordNotFound("File version not found".to_string()))
}

pub async fn find_by_file_id_and_version(
    db: &DatabaseConnection,
    file_id: i32,
    version: i32,
) -> Result<Option<Model>, DbErr> {
    Entity::find()
        .filter(Column::FileId.eq(file_id))
        .filter(Column::Version.eq(version))
        .one(db)
        .await
}

pub async fn find_all_by_file_id(
    db: &DatabaseConnection,
    file_id: i32,
) -> Result<Vec<(Model, Option<super::user::Model>)>, DbErr> {
    Entity::find()
        .find_also_related(super::user::Entity)
        .filter(Column::FileId.eq(file_id))
        .order_by_desc(Column::Version)
        .all(db)
        .await
}

pub async fn exists_by_file_id_and_version(
    db: &DatabaseConnection,
    file_id: i32,
    version: i32,
) -> Result<bool, DbErr> {
    let count = Entity::find()
        .filter(Column::FileId.eq(file_id))
        .filter(Column::Version.eq(version))
        .count(db)
        .await?;
    Ok(count > 0)
}

pub async fn delete_versions_newer_than(
    db: &impl sea_orm::ConnectionTrait,
    file_id: i32,
    version: i32,
) -> Result<u64, DbErr> {
    Entity::delete_many()
        .filter(Column::FileId.eq(file_id))
        .filter(Column::Version.gt(version))
        .exec(db)
        .await
        .map(|res| res.rows_affected)
}

pub async fn get_max_version(db: &DatabaseConnection, file_id: i32) -> Result<Option<i32>, DbErr> {
    use sea_orm::QuerySelect;
    let result = Entity::find()
        .select_only()
        .column_as(Column::Version.max(), "max_version")
        .filter(Column::FileId.eq(file_id))
        .into_tuple::<Option<i32>>()
        .one(db)
        .await?;
    Ok(result.flatten())
}
