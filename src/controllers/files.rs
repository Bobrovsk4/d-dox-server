use axum::{
    Json,
    body::Body,
    extract::{Multipart, Path, State},
    http::{HeaderMap, StatusCode, header},
    response::Response,
    routing::{delete, get, post},
};
use loco_rs::{controller::Routes, prelude::*};
use object_store::{
    Error as ObjectStoreError, ObjectStore,
    aws::{AmazonS3, AmazonS3Builder},
    path::Path as ObjectPath,
};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use crate::models::{file, file_version, user};

#[derive(Debug, Deserialize)]
pub struct UpdateWithVersionRequest {
    pub version: i32,
    pub size: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileVersionInfo {
    pub id: i32,
    pub version: i32,
    pub size: i64,
    pub author: AuthorInfo,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub id: i32,
    pub name: String,
    pub size: i64,
    pub author: AuthorInfo,
    pub created_at: String,
    pub updated_at: String,
    pub version: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthorInfo {
    pub id: i32,
    pub login: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadResponse {
    pub uploaded: Vec<FileInfo>,
}

#[derive(Debug, Deserialize, Clone)]
struct S3Config {
    endpoint: String,
    bucket: String,
    region: String,
    access_key: String,
    secret_key: String,
}

impl Default for S3Config {
    fn default() -> Self {
        Self {
            endpoint: std::env::var("S3_ENDPOINT").unwrap_or_else(|_| "http://minio:9000".into()),
            bucket: std::env::var("S3_BUCKET").unwrap_or_else(|_| "files".into()),
            region: std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".into()),
            access_key: std::env::var("S3_ACCESS_KEY").unwrap_or_else(|_| "admin".into()),
            secret_key: std::env::var("S3_SECRET_KEY").unwrap_or_else(|_| "admin1234".into()),
        }
    }
}

static S3_CONFIG: OnceLock<S3Config> = OnceLock::new();

fn get_s3_config(ctx: &AppContext) -> S3Config {
    S3_CONFIG
        .get_or_init(|| {
            ctx.config
                .settings
                .as_ref()
                .and_then(|s| serde_json::from_value(s.clone()).ok())
                .unwrap_or_default()
        })
        .clone()
}

fn create_s3_store(config: &S3Config) -> Result<AmazonS3> {
    let store = AmazonS3Builder::new()
        .with_bucket_name(&config.bucket)
        .with_region(&config.region)
        .with_endpoint(&config.endpoint)
        .with_access_key_id(&config.access_key)
        .with_secret_access_key(&config.secret_key)
        .with_allow_http(true)
        .with_virtual_hosted_style_request(false)
        .build()
        .map_err(|e| Error::Message(e.to_string()))?;

    Ok(store)
}

pub async fn upload_file(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>> {
    let auth_header = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Message("Missing Authorization header".into()))?;

    let token = auth_header.strip_prefix("Bearer ").unwrap_or(auth_header);

    let claims = crate::controllers::auth::decode_token(token)?;

    let config = get_s3_config(&ctx);
    let store = create_s3_store(&config)?;
    let mut uploaded = Vec::new();

    let user_id: i32 = claims.pid.parse().unwrap_or(0);
    let author = user::find_by_id(&ctx.db, user_id)
        .await?
        .ok_or_else(|| Error::Message("User not found".into()))?;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| Error::Message(format!("Multipart error: {e}")))?
    {
        let file_name = field
            .file_name()
            .map(|s| s.to_string())
            .ok_or_else(|| Error::Message("No filename in multipart field".into()))?;

        let bytes = field
            .bytes()
            .await
            .map_err(|e| Error::Message(format!("Read error: {e}")))?;

        let size = bytes.len() as i64;

        let path = ObjectPath::from(file_name.clone());
        store
            .put(&path, bytes.into())
            .await
            .map_err(|e| Error::Message(format!("Upload failed: {e}")))?;

        let created_file = file::create(&ctx.db, &file_name, size, author.id).await?;
        uploaded.push(FileInfo {
            id: created_file.id,
            name: created_file.name,
            size: created_file.size,
            author: AuthorInfo {
                id: author.id,
                login: author.login.clone(),
            },
            created_at: created_file.created_at.and_utc().to_rfc3339(),
            updated_at: created_file.updated_at.and_utc().to_rfc3339(),
            version: created_file.version,
        });
    }

    Ok(Json(UploadResponse { uploaded }))
}

pub async fn get_all_files(State(ctx): State<AppContext>) -> Result<Json<Vec<FileInfo>>> {
    let db_files = file::find_all_with_authors(&ctx.db).await?;

    let files: Vec<FileInfo> = db_files
        .into_iter()
        .filter_map(|(f, author)| {
            author.map(|a| FileInfo {
                id: f.id,
                name: f.name,
                size: f.size,
                author: AuthorInfo {
                    id: a.id,
                    login: a.login,
                },
                created_at: f.created_at.and_utc().to_rfc3339(),
                updated_at: f.updated_at.and_utc().to_rfc3339(),
                version: f.version,
            })
        })
        .collect();

    Ok(Json(files))
}

pub async fn get_file(
    State(ctx): State<AppContext>,
    Path(file_name): Path<String>,
) -> Result<Response> {
    let config = get_s3_config(&ctx);
    let store = create_s3_store(&config)?;

    let path = ObjectPath::from(file_name.clone());

    let result = store.get(&path).await.map_err(|e| match e {
        ObjectStoreError::NotFound { .. } => Error::NotFound,
        _ => Error::Message(format!("Download error: {e}")),
    })?;

    let content_type = mime_guess::from_path(&file_name)
        .first_or_octet_stream()
        .to_string();

    let bytes = result
        .bytes()
        .await
        .map_err(|e| Error::Message(format!("Read error: {e}")))?;

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", file_name),
        )
        .header(header::CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes))
        .map_err(|e| Error::Message(format!("Build response: {e}")))?;

    Ok(response)
}

pub async fn sync_files(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<FileInfo>> {
    let auth_header = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Message("Missing Authorization header".into()))?;

    let token = auth_header.strip_prefix("Bearer ").unwrap_or(auth_header);

    let claims = crate::controllers::auth::decode_token(token)?;

    let user_id: i32 = claims.pid.parse().unwrap_or(0);
    let author = user::find_by_id(&ctx.db, user_id)
        .await?
        .ok_or_else(|| Error::Message("User not found".into()))?;

    let mut file_id: Option<i32> = None;
    let mut version: Option<i32> = None;
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut file_name: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| Error::Message(format!("Multipart error: {e}")))?
    {
        if let Some(name) = field.name() {
            match name {
                "file_id" => {
                    let text = field
                        .text()
                        .await
                        .map_err(|e| Error::Message(format!("Read file_id: {e}")))?;
                    file_id = text.parse().ok();
                }
                "version" => {
                    let text = field
                        .text()
                        .await
                        .map_err(|e| Error::Message(format!("Read version: {e}")))?;
                    version = text.parse().ok();
                }
                "file" => {
                    file_name = field.file_name().map(|s| s.to_string());
                    let bytes = field
                        .bytes()
                        .await
                        .map_err(|e| Error::Message(format!("Read file: {e}")))?;
                    file_bytes = Some(bytes.to_vec());
                }
                _ => {}
            }
        }
    }

    let file_id = file_id.ok_or_else(|| Error::Message("Missing file_id".into()))?;
    let version = version.ok_or_else(|| Error::Message("Missing version".into()))?;
    let bytes = file_bytes.ok_or_else(|| Error::Message("Missing file".into()))?;
    let file_name = file_name.ok_or_else(|| Error::Message("Missing filename".into()))?;

    let config = get_s3_config(&ctx);
    let store = create_s3_store(&config)?;

    let size = bytes.len() as i64;

    let synced_file = file::sync_with_version_check(&ctx.db, file_id, version, size, author.id)
        .await
        .map_err(|e| {
            if e.to_string().contains("Version conflict") {
                Error::BadRequest(e.to_string())
            } else {
                Error::Message(e.to_string())
            }
        })?;

    let new_version = synced_file.version;

    let versioned_path = ObjectPath::from(format!(
        "versions/{}/v{}/{}",
        synced_file.id, new_version, file_name
    ));
    store
        .put(&versioned_path, bytes.clone().into())
        .await
        .map_err(|e| Error::Message(format!("Upload failed: {e}")))?;

    let latest_path = ObjectPath::from(file_name.clone());
    store
        .put(&latest_path, bytes.into())
        .await
        .map_err(|e| Error::Message(format!("Upload failed: {e}")))?;

    Ok(Json(FileInfo {
        id: synced_file.id,
        name: synced_file.name.clone(),
        size: synced_file.size,
        author: AuthorInfo {
            id: author.id,
            login: author.login.clone(),
        },
        created_at: synced_file.created_at.and_utc().to_rfc3339(),
        updated_at: synced_file.updated_at.and_utc().to_rfc3339(),
        version: synced_file.version,
    }))
}

pub async fn update_file_with_version(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Path(file_id): Path<i32>,
    Json(body): Json<UpdateWithVersionRequest>,
) -> Result<Json<FileInfo>> {
    let auth_header = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Message("Missing Authorization header".into()))?;

    let token = auth_header.strip_prefix("Bearer ").unwrap_or(auth_header);

    let claims = crate::controllers::auth::decode_token(token)?;

    let user_id: i32 = claims.pid.parse().unwrap_or(0);
    let author = user::find_by_id(&ctx.db, user_id)
        .await?
        .ok_or_else(|| Error::Message("User not found".into()))?;

    let updated_file = file::update_with_version_check(&ctx.db, file_id, body.version, body.size)
        .await
        .map_err(|e| {
            if e.to_string().contains("Version conflict") {
                Error::BadRequest(e.to_string())
            } else {
                Error::Message(e.to_string())
            }
        })?;

    Ok(Json(FileInfo {
        id: updated_file.id,
        name: updated_file.name.clone(),
        size: updated_file.size,
        author: AuthorInfo {
            id: author.id,
            login: author.login.clone(),
        },
        created_at: updated_file.created_at.and_utc().to_rfc3339(),
        updated_at: updated_file.updated_at.and_utc().to_rfc3339(),
        version: updated_file.version,
    }))
}

pub async fn get_file_versions(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Path(file_id): Path<i32>,
) -> Result<Json<Vec<FileVersionInfo>>> {
    let auth_header = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Message("Missing Authorization header".into()))?;

    let _token = auth_header.strip_prefix("Bearer ").unwrap_or(auth_header);
    let _claims = crate::controllers::auth::decode_token(_token)?;

    let versions = file_version::find_all_by_file_id(&ctx.db, file_id)
        .await
        .map_err(|e| Error::Message(e.to_string()))?;

    let version_infos: Vec<FileVersionInfo> = versions
        .into_iter()
        .filter_map(|(v, author)| {
            author.map(|a| FileVersionInfo {
                id: v.id,
                version: v.version,
                size: v.size,
                author: AuthorInfo {
                    id: a.id,
                    login: a.login,
                },
                created_at: v.created_at.and_utc().to_rfc3339(),
            })
        })
        .collect();

    Ok(Json(version_infos))
}

pub async fn get_file_version(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Path((file_name, version)): Path<(String, i32)>,
) -> Result<Response> {
    let auth_header = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Message("Missing Authorization header".into()))?;

    let _token = auth_header.strip_prefix("Bearer ").unwrap_or(auth_header);
    let _claims = crate::controllers::auth::decode_token(_token)?;

    let file_record = file::find_by_name(&ctx.db, &file_name)
        .await
        .map_err(|e| Error::Message(e.to_string()))?
        .ok_or_else(|| Error::NotFound)?;

    let _version_record =
        file_version::find_by_file_id_and_version(&ctx.db, file_record.id, version)
            .await
            .map_err(|e| Error::Message(e.to_string()))?
            .ok_or_else(|| Error::NotFound)?;

    let s3_key = format!("versions/{}/v{}/{}", file_record.id, version, file_name);

    let config = get_s3_config(&ctx);
    let store = create_s3_store(&config)?;

    let path = ObjectPath::from(s3_key.clone());

    let result = match store.get(&path).await {
        Ok(r) => Ok(r),
        Err(_) => {
            let fallback_path = ObjectPath::from(file_name.clone());
            store.get(&fallback_path).await.map_err(|e| match e {
                ObjectStoreError::NotFound { .. } => Error::NotFound,
                _ => Error::Message(format!("Download error: {e}")),
            })
        }
    }?;

    let content_type = mime_guess::from_path(&file_name)
        .first_or_octet_stream()
        .to_string();

    let bytes = result
        .bytes()
        .await
        .map_err(|e| Error::Message(format!("Read error: {e}")))?;

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"{}_v{}\"",
                file_name.trim_end_matches(|c: char| !c.is_alphanumeric()),
                version
            ),
        )
        .header(header::CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes))
        .map_err(|e| Error::Message(format!("Build response: {e}")))?;

    Ok(response)
}

pub async fn delete_file(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Path(file_name): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let auth_header = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Message("Missing Authorization header".into()))?;

    let token = auth_header.strip_prefix("Bearer ").unwrap_or(auth_header);
    let _claims = crate::controllers::auth::decode_token(token)?;

    let config = get_s3_config(&ctx);
    let store = create_s3_store(&config)?;

    let file_record = file::find_by_name(&ctx.db, &file_name)
        .await
        .map_err(|e| Error::Message(e.to_string()))?;

    let latest_path = ObjectPath::from(file_name.clone());
    let _ = store.delete(&latest_path).await;

    if let Some(f) = file_record {
        for v in 1..=f.version {
            let versioned_path =
                ObjectPath::from(format!("versions/{}/v{}/{}", f.id, v, file_name));
            let _ = store.delete(&versioned_path).await;
        }
    }

    file::delete_by_name(&ctx.db, &file_name)
        .await
        .map_err(|e| Error::Message(e.to_string()))?;

    Ok(Json(serde_json::json!({ "deleted": file_name })))
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/files")
        .add("", post(upload_file))
        .add("", get(get_all_files))
        .add("/{file_name}", get(get_file))
        .add("/{file_name}", delete(delete_file))
        .add("/sync", post(sync_files))
        .add("/{id}/versions", get(get_file_versions))
        .add("/{file_name}/versions/{version}", get(get_file_version))
}
