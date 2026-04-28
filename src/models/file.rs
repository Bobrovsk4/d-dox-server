use chrono::Utc;
use loco_rs::prelude::*;
use sea_orm::{ActiveValue::NotSet, entity::prelude::*};
use serde::{Deserialize, Serialize};

use super::file_version;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "files")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub size: i64,
    pub author_id: i32,
    #[sea_orm(column_type = "Timestamp")]
    pub created_at: sea_orm::prelude::DateTime,
    #[sea_orm(column_type = "Timestamp")]
    pub updated_at: sea_orm::prelude::DateTime,
    #[sea_orm(column_type = "Integer", default_value = 1)]
    pub version: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::AuthorId",
        to = "super::user::Column::Id"
    )]
    Author,
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Author.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

pub async fn create(
    db: &DatabaseConnection,
    name: &str,
    size: i64,
    author_id: i32,
) -> Result<Model, DbErr> {
    let now = Utc::now().naive_utc();
    let res = Entity::insert(ActiveModel {
        id: NotSet,
        name: Set(name.to_string()),
        size: Set(size),
        author_id: Set(author_id),
        created_at: Set(now),
        updated_at: Set(now),
        version: Set(1),
    })
    .exec(db)
    .await?;

    Entity::find_by_id(res.last_insert_id)
        .one(db)
        .await?
        .ok_or(DbErr::RecordNotFound("File not found".to_string()))
}

pub async fn find_by_name(db: &DatabaseConnection, name: &str) -> Result<Option<Model>, DbErr> {
    Entity::find().filter(Column::Name.eq(name)).one(db).await
}

pub async fn delete_by_name(db: &DatabaseConnection, name: &str) -> Result<(), DbErr> {
    use sea_orm::EntityTrait;

    let file = Entity::find().filter(Column::Name.eq(name)).one(db).await?;
    if let Some(f) = file {
        Entity::delete_by_id(f.id).exec(db).await?;
    }
    Ok(())
}

pub async fn find_all_with_authors(
    db: &DatabaseConnection,
) -> Result<Vec<(Model, Option<super::user::Model>)>, DbErr> {
    Entity::find()
        .find_also_related(super::user::Entity)
        .all(db)
        .await
}

pub async fn find_with_author(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<(Model, Option<super::user::Model>)>, DbErr> {
    Entity::find()
        .find_also_related(super::user::Entity)
        .filter(Column::Id.eq(id))
        .one(db)
        .await
}

pub async fn update_with_version_check(
    db: &DatabaseConnection,
    id: i32,
    expected_version: i32,
    size: i64,
) -> Result<Model, DbErr> {
    use sea_orm::{ActiveModelTrait, EntityTrait};

    let existing = Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or(DbErr::RecordNotFound(format!("File {} not found", id)))?;

    if existing.version != expected_version {
        return Err(DbErr::Custom(format!(
            "Version conflict: expected {}, current {}",
            expected_version, existing.version
        )));
    }

    let now = Utc::now().naive_utc();
    let mut active_model: ActiveModel = existing.clone().into();
    active_model.size = Set(size);
    active_model.updated_at = Set(now);
    active_model.version = Set(existing.version + 1);
    active_model.update(db).await
}

pub async fn sync_with_version_check(
    db: &DatabaseConnection,
    file_id: i32,
    expected_version: i32,
    size: i64,
    author_id: i32,
) -> Result<Model, DbErr> {
    use sea_orm::{ActiveModelTrait, EntityTrait};

    let existing = Entity::find_by_id(file_id)
        .one(db)
        .await?
        .ok_or(DbErr::RecordNotFound(format!("File {} not found", file_id)))?;

    if existing.version != expected_version {
        return Err(DbErr::Custom(format!(
            "Version conflict: expected {}, current {}",
            expected_version, existing.version
        )));
    }

    let new_version = existing.version + 1;
    let now = Utc::now().naive_utc();
    let mut active_model: ActiveModel = existing.clone().into();
    active_model.size = Set(size);
    active_model.updated_at = Set(now);
    active_model.version = Set(new_version);
    let updated = active_model.update(db).await?;

    super::file_version::create(db, file_id, new_version, size, author_id).await?;

    Ok(updated)
}

pub async fn sync_by_name_and_author(
    db: &DatabaseConnection,
    name: &str,
    size: i64,
    author_id: i32,
) -> Result<Model, DbErr> {
    use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter};

    let now = Utc::now().naive_utc();

    if let Some(existing) = Entity::find()
        .filter(Column::Name.eq(name))
        .filter(Column::AuthorId.eq(author_id))
        .one(db)
        .await?
    {
        let mut active_model: ActiveModel = existing.clone().into();
        active_model.size = Set(size);
        active_model.updated_at = Set(now);
        active_model.version = Set(existing.version + 1);
        return active_model.update(db).await;
    }

    Entity::insert(ActiveModel {
        id: NotSet,
        name: Set(name.to_string()),
        size: Set(size),
        author_id: Set(author_id),
        created_at: Set(now),
        updated_at: Set(now),
        version: Set(1),
    })
    .exec(db)
    .await?;

    Entity::find()
        .filter(Column::Name.eq(name))
        .filter(Column::AuthorId.eq(author_id))
        .one(db)
        .await?
        .ok_or(DbErr::RecordNotFound("File not found".to_string()))
}

pub async fn revert_to_version(
    db: &DatabaseConnection,
    file_id: i32,
    target_version: i32,
    _author_id: i32,
) -> Result<Model, DbErr> {
    let txn = db.begin().await?;

    let target_version_record = file_version::Entity::find()
        .filter(file_version::Column::FileId.eq(file_id))
        .filter(file_version::Column::Version.eq(target_version))
        .one(&txn)
        .await?
        .ok_or_else(|| DbErr::Custom(format!("Version {} not found", target_version)))?;

    file_version::delete_versions_newer_than(&txn, file_id, target_version).await?;

    let current_file = Entity::find_by_id(file_id)
        .one(&txn)
        .await?
        .ok_or(DbErr::RecordNotFound(format!("File {} not found", file_id)))?;

    let mut active_file: ActiveModel = current_file.into();
    active_file.version = Set(target_version);
    active_file.size = Set(target_version_record.size);
    active_file.updated_at = Set(Utc::now().naive_utc());
    let updated_file = active_file.update(&txn).await?;

    txn.commit().await?;
    Ok(updated_file)
}
