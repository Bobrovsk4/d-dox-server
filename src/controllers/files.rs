use axum::{
    body::Body,
    extract::{Multipart, Path, State},
    http::{header, StatusCode},
    response::Response,
    routing::{get, post},
    Json,
};
use loco_rs::{controller::Routes, prelude::*};
use object_store::{
    aws::{AmazonS3, AmazonS3Builder},
    path::Path as ObjectPath,
    Error as ObjectStoreError, ObjectStore,
};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use crate::models::{file, user};

#[derive(Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub id: i32,
    pub name: String,
    pub size: i64,
    pub author: AuthorInfo,
    pub created_at: String,
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
    auth: auth::JWT,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>> {
    let config = get_s3_config(&ctx);
    let store = create_s3_store(&config)?;
    let mut uploaded = Vec::new();

    let user_id: i32 = auth.claims.pid.parse().unwrap_or(0);
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
        });
    }

    Ok(Json(UploadResponse { uploaded }))
}

pub async fn get_all_files(State(ctx): State<AppContext>) -> Result<Json<Vec<FileInfo>>> {
    let db_files = file::find_all_with_authors(&ctx.db).await?;

    let files = db_files
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
    
    let result = store
        .get(&path)
        .await
        .map_err(|e| match e {
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

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/files")
        .add("", post(upload_file))
        .add("", get(get_all_files))
        .add("/{file_name}", get(get_file))
}
