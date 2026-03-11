use chrono::Utc;
use loco_rs::prelude::*;
use sea_orm::{entity::prelude::*, ActiveValue::NotSet};
use serde::{Deserialize, Serialize};

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
    let res = Entity::insert(ActiveModel {
        id: NotSet,
        name: Set(name.to_string()),
        size: Set(size),
        author_id: Set(author_id),
        created_at: Set(Utc::now().naive_utc()),
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
