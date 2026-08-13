use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::Serialize;

use crate::{
    app::{AppState, authenticate_at},
    error::ApiError,
    model::{SharedMemo, SharedUrl},
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/latest", get(latest))
}

async fn latest(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Latest>, ApiError> {
    let now = state.now();
    let actor = authenticate_at(&state, &headers, now).await?;
    let url = state
        .repository()
        .get_latest_url(&actor.user_id, now.naive_utc())
        .await
        .map_err(ApiError::internal)?;
    let memo = state
        .repository()
        .get_latest_memo(&actor.user_id, now.naive_utc())
        .await
        .map_err(ApiError::internal)?;

    match (url, memo) {
        (Some(url), Some(memo)) if url.created_at > memo.updated_at => Ok(Json(Latest::Url { url })),
        (Some(_), Some(memo)) => Ok(Json(Latest::Memo { memo })),
        (Some(url), None) => Ok(Json(Latest::Url { url })),
        (None, Some(memo)) => Ok(Json(Latest::Memo { memo })),
        (None, None) => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "LATEST_NOT_FOUND",
            "no unexpired URL or memo was found",
        )),
    }
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Latest {
    Url { url: SharedUrl },
    Memo { memo: SharedMemo },
}
