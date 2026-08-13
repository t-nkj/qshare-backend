use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
    routing::{get, post},
};
use chrono::Duration;
use image::ImageReader;
use serde::Serialize;
use tokio::{fs::OpenOptions, io::AsyncWriteExt};
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::{
    app::{AppState, ListQuery, authenticate, authenticate_at, json_body, require_uuid},
    error::ApiError,
    model::SharedFile,
    repository::CreateFile,
    validation,
};

const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024;
const MAX_USER_FILE_SIZE: u64 = 1024 * 1024 * 1024;
const MULTIPART_BODY_LIMIT: usize = (MAX_USER_FILE_SIZE as usize) + (16 * 1024 * 1024);
const THUMBNAIL_MAX_DIMENSION: u32 = 512;
const THUMBNAIL_MAX_SIZE: usize = 512 * 1024;
const THUMBNAIL_DIMENSIONS: [u32; 5] = [512, 384, 256, 192, 128];
const THUMBNAIL_QUALITIES: [f32; 5] = [85.0, 75.0, 65.0, 50.0, 35.0];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/files", post(create_files).get(list_files))
        .route(
            "/v1/files/{file_id}",
            get(download_file).patch(rename_file).delete(delete_file),
        )
        .route("/v1/files/{file_id}/thumbnail", get(download_thumbnail))
        .layer(DefaultBodyLimit::max(MULTIPART_BODY_LIMIT))
}

async fn create_files(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<FileUploadResult>), ApiError> {
    let now = state.now();
    let actor = authenticate_at(&state, &headers, now).await?;
    let mut created = Vec::new();
    let mut failed = Vec::new();
    let mut total_size = 0_u64;
    let mut index = 0_usize;
    let upload_id = Uuid::new_v4().to_string();

    while let Some(mut field) = multipart.next_field().await.map_err(multipart_error)? {
        let original_name = field.file_name().map(ToOwned::to_owned);
        let failure = if field.name() != Some("files") {
            drain_field(&mut field).await?;
            Some(UploadFailure::invalid_multipart("each part must use the files field"))
        } else if original_name.is_none() {
            drain_field(&mut field).await?;
            Some(UploadFailure::invalid_multipart("a filename is required"))
        } else if total_size >= MAX_USER_FILE_SIZE {
            drain_field(&mut field).await?;
            Some(UploadFailure::total_size_exceeded())
        } else {
            let original_name = original_name.as_deref().expect("checked above");
            match validation::file_name(original_name) {
                Ok(name) => {
                    let content_type = field
                        .content_type()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "application/octet-stream".to_owned());
                    let storage_key = Uuid::new_v4().to_string();
                    let temporary_path = state.file_storage_dir().join(format!(".{storage_key}.upload"));
                    let stored_path = state.file_storage_dir().join(&storage_key);
                    let mut output = match OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&temporary_path)
                        .await
                    {
                        Ok(output) => Some(output),
                        Err(error) => {
                            tracing::error!(%error, "failed to create temporary shared file");
                            None
                        }
                    };
                    let mut size = 0_u64;
                    let mut failure = output.is_none().then_some(UploadFailure::save_failed());

                    while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
                        let chunk_size = u64::try_from(chunk.len()).expect("chunk length fits in u64");
                        size = size.saturating_add(chunk_size);
                        total_size = total_size.saturating_add(chunk_size);
                        if failure.is_none() && size > MAX_FILE_SIZE {
                            failure = Some(UploadFailure::file_too_large());
                        }
                        if failure.is_none() && total_size > MAX_USER_FILE_SIZE {
                            failure = Some(UploadFailure::total_size_exceeded());
                        }
                        if let Some(output) = output.as_mut()
                            && failure.is_none()
                            && let Err(error) = output.write_all(&chunk).await
                        {
                            tracing::error!(%error, "failed to write temporary shared file");
                            failure = Some(UploadFailure::save_failed());
                        }
                    }
                    drop(field);
                    if let Some(failure) = failure {
                        drop(output);
                        let _ = tokio::fs::remove_file(&temporary_path).await;
                        Some(failure)
                    } else {
                        let output = output.expect("output exists without a failure");
                        let flush_result = output.sync_all().await;
                        drop(output);
                        if let Err(error) = flush_result {
                            tracing::error!(%error, "failed to flush temporary shared file");
                            let _ = tokio::fs::remove_file(&temporary_path).await;
                            Some(UploadFailure::save_failed())
                        } else if let Err(error) = tokio::fs::rename(&temporary_path, &stored_path).await {
                            tracing::error!(%error, "failed to store shared file");
                            let _ = tokio::fs::remove_file(&temporary_path).await;
                            Some(UploadFailure::save_failed())
                        } else {
                            let file_now = state.now();
                            let thumbnail = create_thumbnail(&state, &stored_path, &content_type).await;
                            match state
                                .repository()
                                .create_file_and_evict(
                                    CreateFile {
                                        id: Uuid::new_v4().to_string(),
                                        upload_id: upload_id.clone(),
                                        user_id: actor.user_id.clone(),
                                        source_device_id: actor.id.clone(),
                                        source_device_name: actor.name.clone(),
                                        name,
                                        content_type,
                                        size,
                                        storage_key: storage_key.clone(),
                                        thumbnail,
                                        now: file_now.naive_utc(),
                                        expires_at: (file_now + Duration::days(3)).naive_utc(),
                                    },
                                    MAX_USER_FILE_SIZE,
                                )
                                .await
                            {
                                Ok(result) => {
                                    for evicted in result.evicted {
                                        let _ =
                                            tokio::fs::remove_file(state.file_storage_dir().join(evicted.storage_key))
                                                .await;
                                    }
                                    created.push(result.file);
                                    None
                                }
                                Err(error) => {
                                    tracing::error!(%error, "failed to save shared file metadata");
                                    let _ = tokio::fs::remove_file(&stored_path).await;
                                    Some(UploadFailure::save_failed())
                                }
                            }
                        }
                    }
                }
                Err(_) => {
                    drain_field(&mut field).await?;
                    Some(UploadFailure::invalid_file_name())
                }
            }
        };

        if let Some(error) = failure {
            failed.push(FailedFile {
                index,
                name: original_name,
                error,
            });
        }
        index += 1;
    }

    if !created.is_empty() {
        return Ok((StatusCode::CREATED, Json(FileUploadResult { created, failed })));
    }
    let first_failure = failed
        .first()
        .ok_or_else(|| ApiError::bad_request("INVALID_MULTIPART", "a files field is required"))?;
    let status = if failed.iter().all(|failure| failure.error.is_size_error()) {
        StatusCode::PAYLOAD_TOO_LARGE
    } else {
        StatusCode::BAD_REQUEST
    };
    Err(ApiError::new(
        status,
        first_failure.error.code,
        first_failure.error.message,
    ))
}

async fn drain_field(field: &mut axum::extract::multipart::Field<'_>) -> Result<(), ApiError> {
    while field.chunk().await.map_err(multipart_error)?.is_some() {}
    Ok(())
}

async fn list_files(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
    headers: HeaderMap,
) -> Result<Json<Files>, ApiError> {
    let now = state.now();
    let actor = authenticate_at(&state, &headers, now).await?;
    let limit = validation::limit(query.limit.as_deref())?;
    let cursor = validation::decode_cursor(query.cursor.as_deref())?;
    let mut files = state
        .repository()
        .list_files(&actor.user_id, now.naive_utc(), limit, cursor.as_ref())
        .await
        .map_err(ApiError::internal)?;
    let has_more = files.len() > limit as usize;
    if has_more {
        files.truncate(limit as usize);
    }
    let next_cursor = has_more
        .then(|| {
            files
                .last()
                .map(|file| validation::encode_cursor(&file.id, file.created_at))
        })
        .flatten();
    Ok(Json(Files { files, next_cursor }))
}

async fn download_file(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let now = state.now();
    let actor = authenticate_at(&state, &headers, now).await?;
    require_uuid(&file_id, "FILE_NOT_FOUND", "file was not found")?;
    let Some(record) = state
        .repository()
        .get_file(&actor.user_id, &file_id, now.naive_utc())
        .await
        .map_err(ApiError::internal)?
    else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "FILE_NOT_FOUND",
            "file was not found",
        ));
    };
    let path = state.file_storage_dir().join(&record.storage_key);
    let input = tokio::fs::File::open(&path).await.map_err(|error| {
        tracing::warn!(%error, file_id, "shared file content is missing");
        ApiError::new(
            StatusCode::NOT_FOUND,
            "FILE_CONTENT_NOT_FOUND",
            "file content is not available",
        )
    })?;
    let disposition = format!(
        "attachment; filename*=UTF-8''{}",
        url::form_urlencoded::byte_serialize(record.file.name.as_bytes()).collect::<String>()
    );
    let mut response = Response::new(Body::from_stream(ReaderStream::new(input)));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&record.file.content_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&record.file.size.to_string()).expect("size header is valid"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition).expect("encoded disposition is valid"),
    );
    Ok(response)
}

async fn download_thumbnail(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let now = state.now();
    let actor = authenticate_at(&state, &headers, now).await?;
    require_uuid(&file_id, "FILE_NOT_FOUND", "file was not found")?;
    let thumbnail = state
        .repository()
        .get_file_thumbnail(&actor.user_id, &file_id, now.naive_utc())
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "THUMBNAIL_NOT_AVAILABLE",
                "thumbnail is not available",
            )
        })?;
    let mut response = Response::new(Body::from(thumbnail.data));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&thumbnail.content_type).expect("stored thumbnail content type is valid"),
    );
    Ok(response)
}

async fn create_thumbnail(state: &AppState, path: &std::path::Path, content_type: &str) -> Option<Vec<u8>> {
    if !content_type.to_ascii_lowercase().starts_with("image/") {
        return None;
    }
    let _permit = state.acquire_thumbnail_generation().await;
    let path = path.to_owned();
    let image_path = path.clone();
    match tokio::task::spawn_blocking(move || encode_thumbnail(&image_path)).await {
        Ok(Ok(thumbnail)) => Some(thumbnail),
        Ok(Err(error)) => {
            tracing::warn!(%error, path = %path.display(), "failed to create image thumbnail");
            None
        }
        Err(error) => {
            tracing::error!(%error, "image thumbnail task failed");
            None
        }
    }
}

fn encode_thumbnail(path: &std::path::Path) -> Result<Vec<u8>, image::ImageError> {
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8_192);
    limits.max_image_height = Some(8_192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let source = reader.decode()?;
    for dimension in THUMBNAIL_DIMENSIONS {
        let image = source
            .thumbnail(
                dimension.min(THUMBNAIL_MAX_DIMENSION),
                dimension.min(THUMBNAIL_MAX_DIMENSION),
            )
            .to_rgba8();
        let encoder = webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height());
        for quality in THUMBNAIL_QUALITIES {
            let thumbnail = encoder.encode(quality).to_vec();
            if thumbnail.len() <= THUMBNAIL_MAX_SIZE {
                return Ok(thumbnail);
            }
        }
    }
    Err(image::ImageError::Limits(image::error::LimitError::from_kind(
        image::error::LimitErrorKind::InsufficientMemory,
    )))
}

async fn rename_file(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<FileEnvelope>, ApiError> {
    let now = state.now();
    let actor = authenticate_at(&state, &headers, now).await?;
    require_uuid(&file_id, "FILE_NOT_FOUND", "file was not found")?;
    let value = json_body(&headers, &body)?;
    let name = value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ApiError::bad_request("INVALID_FILE_NAME", "name must be a string"))?;
    let name = validation::file_name(name)?;
    let file = state
        .repository()
        .rename_file(
            &actor.user_id,
            &file_id,
            &name,
            now.naive_utc(),
            (now + Duration::days(3)).naive_utc(),
        )
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "FILE_NOT_FOUND", "file was not found"))?;
    Ok(Json(FileEnvelope { file }))
}

async fn delete_file(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let actor = authenticate(&state, &headers).await?;
    require_uuid(&file_id, "FILE_NOT_FOUND", "file was not found")?;
    let Some(record) = state
        .repository()
        .delete_file(&actor.user_id, &file_id)
        .await
        .map_err(ApiError::internal)?
    else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "FILE_NOT_FOUND",
            "file was not found",
        ));
    };
    match tokio::fs::remove_file(state.file_storage_dir().join(record.storage_key)).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => tracing::error!(%error, file_id, "failed to delete shared file content"),
    }
    Ok(StatusCode::NO_CONTENT)
}

fn multipart_error(error: axum::extract::multipart::MultipartError) -> ApiError {
    tracing::debug!(%error, "invalid multipart request");
    ApiError::bad_request("INVALID_MULTIPART", "multipart request is invalid")
}

#[derive(Serialize)]
struct FileEnvelope {
    file: SharedFile,
}

#[derive(Serialize)]
struct FileUploadResult {
    created: Vec<SharedFile>,
    failed: Vec<FailedFile>,
}

#[derive(Serialize)]
struct FailedFile {
    index: usize,
    name: Option<String>,
    error: UploadFailure,
}

#[derive(Serialize)]
struct UploadFailure {
    code: &'static str,
    message: &'static str,
}

impl UploadFailure {
    fn invalid_multipart(message: &'static str) -> Self {
        Self {
            code: "INVALID_MULTIPART",
            message,
        }
    }

    fn invalid_file_name() -> Self {
        Self {
            code: "INVALID_FILE_NAME",
            message: "file name is invalid",
        }
    }

    fn file_too_large() -> Self {
        Self {
            code: "FILE_TOO_LARGE",
            message: "file must not exceed 100 MiB",
        }
    }

    fn total_size_exceeded() -> Self {
        Self {
            code: "TOTAL_SIZE_EXCEEDED",
            message: "total file size must not exceed 1 GiB",
        }
    }

    fn save_failed() -> Self {
        Self {
            code: "FILE_SAVE_FAILED",
            message: "file could not be saved",
        }
    }

    fn is_size_error(&self) -> bool {
        matches!(self.code, "FILE_TOO_LARGE" | "TOTAL_SIZE_EXCEEDED")
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Files {
    files: Vec<SharedFile>,
    next_cursor: Option<String>,
}
