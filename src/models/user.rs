use loco_rs::prelude::*;
use sea_orm::{entity::prelude::*, ActiveValue::NotSet};
use serde::{Deserialize, Serialize};

use crate::models::role;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub username: String,
    pub login: String,
    pub password: String,
    pub role_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "role::Entity",
        from = "Column::RoleId",
        to = "role::Column::Id"
    )]
    Role,
}

impl Related<role::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Role.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

pub async fn create(
    db: &DatabaseConnection,
    username: &str,
    login: &str,
    password: &str,
    role_id: i32,
) -> Result<Model, DbErr> {
    let res = Entity::insert(ActiveModel {
        id: NotSet,
        username: Set(username.to_string()),
        login: Set(login.to_string()),
        password: Set(password.to_string()),
        role_id: Set(role_id),
    })
    .exec(db)
    .await?;

    Entity::find_by_id(res.last_insert_id)
        .one(db)
        .await?
        .ok_or(DbErr::RecordNotFound("User not found".to_string()))
}

pub async fn find_by_login(db: &DatabaseConnection, login: &str) -> Result<Option<Model>, DbErr> {
    Entity::find().filter(Column::Login.eq(login)).one(db).await
}

pub async fn find_by_id(db: &DatabaseConnection, id: i32) -> Result<Option<Model>, DbErr> {
    Entity::find_by_id(id).one(db).await
}

pub async fn find_by_username(
    db: &DatabaseConnection,
    username: &str,
) -> Result<Option<Model>, DbErr> {
    Entity::find()
        .filter(Column::Username.eq(username))
        .one(db)
        .await
}

pub async fn find_all_with_roles(
    db: &DatabaseConnection,
) -> Result<Vec<(Model, Option<role::Model>)>, DbErr> {
    Entity::find().find_also_related(role::Entity).all(db).await
}

pub async fn find_with_role(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<(Model, Option<role::Model>)>, DbErr> {
    Entity::find()
        .find_also_related(role::Entity)
        .filter(Column::Id.eq(id))
        .one(db)
        .await
}
