use axum::{
    http::StatusCode,
    routing::{get, post},
};
use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use loco_rs::controller::Routes;
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::models::{role, user};

const JWT_SECRET: &str = "v7SWenu8m9aPQuDkL6pw";
const TOKEN_LIFETIME_HOURS: i64 = 168; // 7 дней

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub pid: String,
    pub login: String,
    pub exp: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RegisterRequest {
    pub username: String,
    pub login: String,
    pub password: String,
    pub role_name: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LoginRequest {
    pub login: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserResponse,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: i32,
    pub username: String,
    pub login: String,
    pub role: Option<RoleResponse>,
}

#[derive(Debug, Serialize)]
pub struct RoleResponse {
    pub id: i32,
    pub name: String,
}

pub async fn register(
    State(ctx): State<AppContext>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Response> {
    if user::find_by_login(&ctx.db, &payload.login)
        .await?
        .is_some()
    {
        return Ok(format::json(("Already exists",)).into_response());
    }

    let role_name = payload.role_name.unwrap_or_else(|| "dummy".to_string());
    let role = match role::find_by_name(&ctx.db, &role_name).await? {
        Some(r) => r,
        None => role::create(&ctx.db, &role_name, serde_json::json!(["read"])).await?,
    };

    let password_hash = hash_password(&payload.password)?;

    let created_user = user::create(
        &ctx.db,
        &payload.username,
        &payload.login,
        &password_hash,
        role.id,
    )
    .await?;

    Ok(format::json((created_user,)).into_response())
}

pub async fn login(
    State(ctx): State<AppContext>,
    Json(payload): Json<LoginRequest>,
) -> Result<Response> {
    let found_user = match user::find_by_login(&ctx.db, &payload.login).await? {
        Some(u) => u,
        None => {
            println!("User {} not found.", &payload.login);
            return Ok((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Invalid login or password"})),
            )
                .into_response());
        }
    };

    if !verify_password(&payload.password, &found_user.password)? {
        println!("Wrong password. Got: {}", &payload.password);
        return Ok((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid login or password"})),
        )
            .into_response());
    }

    let user_role = role::Entity::find_by_id(found_user.role_id)
        .one(&ctx.db)
        .await?;

    let token = generate_token(&found_user.id.to_string(), &found_user.login)?;

    let response = AuthResponse {
        token,
        user: UserResponse {
            id: found_user.id,
            username: found_user.username,
            login: found_user.login,
            role: user_role.map(|r| RoleResponse {
                id: r.id,
                name: r.name,
            }),
        },
    };

    Ok((StatusCode::OK, Json(json!(response))).into_response())
}

pub async fn logout() -> Result<Response> {
    Ok(format::json(("ok",)).into_response())
}

pub async fn me(State(ctx): State<AppContext>, auth: auth::JWT) -> Result<Response> {
    let user_id: i32 = auth.claims.pid.parse().unwrap_or(0);

    match user::find_with_role(&ctx.db, user_id).await? {
        Some((u, role)) => {
            let response = UserResponse {
                id: u.id,
                username: u.username,
                login: u.login,
                role: role.map(|r| RoleResponse {
                    id: r.id,
                    name: r.name,
                }),
            };
            Ok(format::json(response).into_response())
        }
        None => Ok(format::json(("User not found",)).into_response()),
    }
}

fn hash_password(password: &str) -> Result<String> {
    bcrypt::hash(password, bcrypt::DEFAULT_COST).map_err(|e| Error::Message(e.to_string()))
}

fn verify_password(password: &str, hash: &str) -> Result<bool> {
    bcrypt::verify(password, hash).map_err(|e| Error::Message(e.to_string()))
}

fn generate_token(user_id: &str, login: &str) -> Result<String> {
    let expiration = Utc::now()
        .checked_add_signed(Duration::hours(TOKEN_LIFETIME_HOURS))
        .expect("valid timestamp")
        .timestamp() as usize;

    let claims = Claims {
        pid: user_id.to_string(),
        login: login.to_string(),
        exp: expiration,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(JWT_SECRET.as_bytes()),
    )
    .map_err(|e| Error::Message(e.to_string()))
}

pub fn decode_token(token: &str) -> Result<Claims> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(JWT_SECRET.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|e| Error::Message(e.to_string()))
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/auth")
        .add("/register", post(register))
        .add("/login", post(login))
        .add("/logout", post(logout))
        .add("/me", get(me))
}
