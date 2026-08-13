use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
    routing::{get, post},
};
use chrono::Duration;
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
const MULTIPART_BODY_LIMIT: usize = (MAX_FILE_SIZE as usize) + (1024 * 1024);

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/files", post(create_file).get(list_files))
        .route(
            "/v1/files/{file_id}",
            get(download_file).patch(rename_file).delete(delete_file),
        )
        .layer(DefaultBodyLimit::max(MULTIPART_BODY_LIMIT))
}

async fn create_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<FileEnvelope>), ApiError> {
    let now = state.now();
    let actor = authenticate_at(&state, &headers, now).await?;
    let Some(mut field) = multipart.next_field().await.map_err(multipart_error)? else {
        return Err(ApiError::bad_request("INVALID_MULTIPART", "a file field is required"));
    };
    if field.name() != Some("file") || field.file_name().is_none() {
        return Err(ApiError::bad_request(
            "INVALID_MULTIPART",
            "a single file field is required",
        ));
    }
    let name = validation::file_name(field.file_name().expect("checked file name"))?;
    let content_type = field
        .content_type()
        .map(ToString::to_string)
        .unwrap_or_else(|| "application/octet-stream".to_owned());
    let id = Uuid::new_v4().to_string();
    let storage_key = Uuid::new_v4().to_string();
    let temporary_path = state.file_storage_dir().join(format!(".{storage_key}.upload"));
    let stored_path = state.file_storage_dir().join(&storage_key);
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary_path)
        .await
        .map_err(ApiError::internal)?;
    let mut size = 0_u64;
    while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
        size += u64::try_from(chunk.len()).expect("chunk length fits in u64");
        if size > MAX_FILE_SIZE {
            let _ = tokio::fs::remove_file(&temporary_path).await;
            return Err(ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "FILE_TOO_LARGE",
                "file must not exceed 100 MiB",
            ));
        }
        output.write_all(&chunk).await.map_err(ApiError::internal)?;
    }
    drop(field);
    if multipart.next_field().await.map_err(multipart_error)?.is_some() {
        let _ = tokio::fs::remove_file(&temporary_path).await;
        return Err(ApiError::bad_request(
            "INVALID_MULTIPART",
            "only one file field is allowed",
        ));
    }
    output.flush().await.map_err(ApiError::internal)?;
    drop(output);
    tokio::fs::rename(&temporary_path, &stored_path)
        .await
        .map_err(ApiError::internal)?;

    let created = state
        .repository()
        .create_file_and_evict(
            CreateFile {
                id,
                user_id: actor.user_id,
                source_device_id: actor.id,
                source_device_name: actor.name,
                name,
                content_type,
                size,
                storage_key: storage_key.clone(),
                now: now.naive_utc(),
                expires_at: (now + Duration::days(3)).naive_utc(),
            },
            MAX_USER_FILE_SIZE,
        )
        .await;
    let created = match created {
        Ok(created) => created,
        Err(error) => {
            let _ = tokio::fs::remove_file(&stored_path).await;
            return Err(ApiError::internal(error));
        }
    };
    for evicted in created.evicted {
        let _ = tokio::fs::remove_file(state.file_storage_dir().join(evicted.storage_key)).await;
    }
    Ok((StatusCode::CREATED, Json(FileEnvelope { file: created.file })))
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
#[serde(rename_all = "camelCase")]
struct Files {
    files: Vec<SharedFile>,
    next_cursor: Option<String>,
}
