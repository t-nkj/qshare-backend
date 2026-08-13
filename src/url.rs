use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use chrono::Duration;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    app::{AppState, ListQuery, authenticate, authenticate_at, json_body, require_uuid},
    error::ApiError,
    model::SharedUrl,
    repository::CreateUrl,
    validation,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/urls", post(create_url).get(list_urls))
        .route("/v1/urls/{url_id}", axum::routing::delete(delete_url))
}

async fn create_url(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    let now = state.now();
    let actor = authenticate_at(&state, &headers, now).await?;
    let url = validation::http_url(&json_body(&headers, &body)?)?;
    let id = Uuid::new_v4().to_string();
    let shared_url = state
        .repository()
        .create_url(CreateUrl {
            id: &id,
            user_id: &actor.user_id,
            source_device_id: &actor.id,
            source_device_name: &actor.name,
            url: &url,
            now: now.naive_utc(),
            expires_at: (now + Duration::days(7)).naive_utc(),
        })
        .await
        .map_err(ApiError::internal)?;
    Ok((StatusCode::CREATED, Json(UrlEnvelope { url: shared_url })))
}

async fn list_urls(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
    headers: HeaderMap,
) -> Result<Json<Urls>, ApiError> {
    let now = state.now();
    let actor = authenticate_at(&state, &headers, now).await?;
    let limit = validation::limit(query.limit.as_deref())?;
    let cursor = validation::decode_cursor(query.cursor.as_deref())?;
    let mut urls = state
        .repository()
        .list_urls(&actor.user_id, now.naive_utc(), limit, cursor.as_ref())
        .await
        .map_err(ApiError::internal)?;
    let has_more = urls.len() > limit as usize;
    if has_more {
        urls.truncate(limit as usize);
    }
    let next_cursor = has_more
        .then(|| {
            urls.last()
                .map(|url| validation::encode_cursor(&url.id, url.created_at))
        })
        .flatten();
    Ok(Json(Urls { urls, next_cursor }))
}

async fn delete_url(
    State(state): State<AppState>,
    Path(url_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let actor = authenticate(&state, &headers).await?;
    require_uuid(&url_id, "URL_NOT_FOUND", "URL was not found")?;
    let deleted = state
        .repository()
        .delete_url(&actor.user_id, &url_id)
        .await
        .map_err(ApiError::internal)?;
    if !deleted {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "URL_NOT_FOUND",
            "URL was not found",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct UrlEnvelope {
    url: SharedUrl,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Urls {
    urls: Vec<SharedUrl>,
    next_cursor: Option<String>,
}
