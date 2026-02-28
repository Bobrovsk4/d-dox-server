use axum::{
    body::Body,
    extract::{Multipart, Path, State},
    http::StatusCode,
    response::Response,
    routing::{get, post},
    Json,
};
use futures_util::StreamExt;
use loco_rs::{controller::Routes, prelude::*};
use object_store::{aws::AmazonS3, aws::AmazonS3Builder, path::Path as ObjectPath, ObjectStore};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub size: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadResponse {
    pub name: String,
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
            endpoint: "http://minio:9000".to_string(),
            bucket: "files".to_string(),
            region: "us-east-1".to_string(),
            access_key: "minioadmin".to_string(),
            secret_key: "minioadmin".to_string(),
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
    mut multipart: Multipart,
) -> Result<Response> {
    let config = get_s3_config(&ctx);
    let store = create_s3_store(&config)?;

    let mut uploaded_files: Vec<String> = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| Error::Message(e.to_string()))?
    {
        let file_name = field
            .file_name()
            .map(|s| s.to_string())
            .ok_or_else(|| Error::Message("No file name provided".to_string()))?;

        let bytes = field
            .bytes()
            .await
            .map_err(|e| Error::Message(e.to_string()))?;

        let object_path = ObjectPath::from(file_name.clone());
        store
            .put(&object_path, bytes.into())
            .await
            .map_err(|e| Error::Message(e.to_string()))?;

        uploaded_files.push(file_name);
    }

    Ok(Json(serde_json::json!({
        "uploaded": uploaded_files
    }))
    .into_response())
}

pub async fn get_file(
    State(ctx): State<AppContext>,
    Path(params): Path<std::collections::HashMap<String, String>>,
) -> Result<Response> {
    let config = get_s3_config(&ctx);
    let store = create_s3_store(&config)?;

    let file_name = params.get("file_name").ok_or_else(|| Error::Message("File name required".to_string()))?;
    let object_path = ObjectPath::from(file_name.clone());
    let result = store
        .get(&object_path)
        .await
        .map_err(|e| Error::Message(e.to_string()))?;

    let bytes = result
        .bytes()
        .await
        .map_err(|e| Error::Message(e.to_string()))?;

    let response = Response::builder()
        .status(StatusCode::OK)
        .body(Body::from(bytes.to_vec()))
        .map_err(|e| Error::Message(e.to_string()))?;

    Ok(response)
}

pub async fn get_all_files(State(ctx): State<AppContext>) -> Result<Response> {
    let config = get_s3_config(&ctx);
    let store = create_s3_store(&config)?;

    let mut files: Vec<FileInfo> = Vec::new();

    let mut stream = store.list(None);

    while let Some(result) = stream.next().await {
        let meta = result.map_err(|e| Error::Message(e.to_string()))?;
        files.push(FileInfo {
            name: meta.location.filename().unwrap_or("unknown").to_string(),
            size: meta.size,
        });
    }

    Ok(Json(files).into_response())
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/files")
        .add("", get(get_all_files))
        .add("/{file_name}", get(get_file))
        .add("", post(upload_file))
}
