use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, patch, post},
};
use chrono::Duration;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    app::{AppState, ListQuery, authenticate, authenticate_at, json_body, require_uuid},
    error::ApiError,
    model::{SharedMemo, SharedUrl},
    repository::{CreateMemo, CreateMemoBundle, CreateUrlOwned, CreatedMemoItem},
    validation,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/memos", post(create_memo).get(list_memos))
        .route("/v1/memos/latest", get(latest_memo))
        .route("/v1/memos/{memo_id}", patch(update_memo).delete(delete_memo))
}

async fn create_memo(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    let now = state.now();
    let actor = authenticate_at(&state, &headers, now).await?;
    let body = json_body(&headers, &body)?;
    let content = validation::memo_content(&body)?;
    let auto_detect_urls = validation::auto_detect_urls(&body)?;
    let urls = if auto_detect_urls {
        validation::extract_http_urls(&content)
    } else {
        Vec::new()
    };
    let is_url_only = auto_detect_urls && validation::is_bare_http_url(&content, &urls);
    let expires_at = (now + Duration::days(7)).naive_utc();
    let bundle = CreateMemoBundle {
        urls: urls
            .into_iter()
            .map(|url| CreateUrlOwned {
                id: Uuid::new_v4().to_string(),
                user_id: actor.user_id.clone(),
                source_device_id: actor.id.clone(),
                source_device_name: actor.name.clone(),
                url,
                now: now.naive_utc(),
                expires_at,
            })
            .collect(),
        memo: (!is_url_only).then(|| CreateMemo {
            id: Uuid::new_v4().to_string(),
            user_id: actor.user_id,
            source_device_id: actor.id,
            source_device_name: actor.name,
            content,
            now: now.naive_utc(),
            expires_at,
        }),
    };
    let created = state
        .repository()
        .create_memo_bundle(bundle)
        .await
        .map_err(ApiError::internal)?
        .into_iter()
        .map(Created::from)
        .collect();
    Ok((StatusCode::CREATED, Json(MemoCreated { created })))
}

async fn latest_memo(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<MemoEnvelope>, ApiError> {
    let now = state.now();
    let actor = authenticate_at(&state, &headers, now).await?;
    let memo = state
        .repository()
        .get_latest_memo(&actor.user_id, now.naive_utc())
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "MEMO_NOT_FOUND", "no unexpired memo was found"))?;
    Ok(Json(MemoEnvelope { memo }))
}

async fn list_memos(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
    headers: HeaderMap,
) -> Result<Json<Memos>, ApiError> {
    let now = state.now();
    let actor = authenticate_at(&state, &headers, now).await?;
    let limit = validation::limit(query.limit.as_deref())?;
    let cursor = validation::decode_cursor(query.cursor.as_deref())?;
    let mut memos = state
        .repository()
        .list_memos(&actor.user_id, now.naive_utc(), limit, cursor.as_ref())
        .await
        .map_err(ApiError::internal)?;
    let has_more = memos.len() > limit as usize;
    if has_more {
        memos.truncate(limit as usize);
    }
    let next_cursor = has_more
        .then(|| {
            memos
                .last()
                .map(|memo| validation::encode_cursor(&memo.id, memo.created_at))
        })
        .flatten();
    Ok(Json(Memos { memos, next_cursor }))
}

async fn update_memo(
    State(state): State<AppState>,
    Path(memo_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<MemoEnvelope>, ApiError> {
    let now = state.now();
    let actor = authenticate_at(&state, &headers, now).await?;
    require_uuid(&memo_id, "MEMO_NOT_FOUND", "memo was not found")?;
    let content = validation::memo_content(&json_body(&headers, &body)?)?;
    let memo = state
        .repository()
        .update_memo(
            &actor.user_id,
            &memo_id,
            &content,
            now.naive_utc(),
            (now + Duration::days(7)).naive_utc(),
        )
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "MEMO_NOT_FOUND", "memo was not found"))?;
    Ok(Json(MemoEnvelope { memo }))
}

async fn delete_memo(
    State(state): State<AppState>,
    Path(memo_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let actor = authenticate(&state, &headers).await?;
    require_uuid(&memo_id, "MEMO_NOT_FOUND", "memo was not found")?;
    let deleted = state
        .repository()
        .delete_memo(&actor.user_id, &memo_id)
        .await
        .map_err(ApiError::internal)?;
    if !deleted {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "MEMO_NOT_FOUND",
            "memo was not found",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct MemoCreated {
    created: Vec<Created>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Created {
    Url { url: SharedUrl },
    Memo { memo: SharedMemo },
}

impl From<CreatedMemoItem> for Created {
    fn from(value: CreatedMemoItem) -> Self {
        match value {
            CreatedMemoItem::Url(url) => Self::Url { url },
            CreatedMemoItem::Memo(memo) => Self::Memo { memo },
        }
    }
}

#[derive(Serialize)]
struct MemoEnvelope {
    memo: SharedMemo,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Memos {
    memos: Vec<SharedMemo>,
    next_cursor: Option<String>,
}
