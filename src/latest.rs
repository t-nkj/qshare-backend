use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::Serialize;

use crate::{
    app::{AppState, authenticate_at},
    error::ApiError,
    model::{SharedFile, SharedMemo, SharedUrl},
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/latest/{types}", get(latest))
}

async fn latest(
    State(state): State<AppState>,
    Path(types): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Latest>, ApiError> {
    let types = LatestTypes::parse(&types)?;
    latest_for_types(state, types, headers).await
}

async fn latest_for_types(state: AppState, types: LatestTypes, headers: HeaderMap) -> Result<Json<Latest>, ApiError> {
    let now = state.now();
    let actor = authenticate_at(&state, &headers, now).await?;
    let url = if types.url {
        state
            .repository()
            .get_latest_url(&actor.user_id, now.naive_utc())
            .await
            .map_err(ApiError::internal)?
    } else {
        None
    };
    let memo = if types.memo {
        state
            .repository()
            .get_latest_memo(&actor.user_id, now.naive_utc())
            .await
            .map_err(ApiError::internal)?
    } else {
        None
    };
    let file = if types.file {
        state
            .repository()
            .get_latest_file(&actor.user_id, now.naive_utc())
            .await
            .map_err(ApiError::internal)?
    } else {
        None
    };

    let mut latest = Vec::new();
    if let Some(url) = url {
        latest.push((url.created_at, 2_u8, LatestCandidate::Url { url }));
    }
    if let Some(memo) = memo {
        latest.push((memo.updated_at, 3_u8, LatestCandidate::Memo { memo }));
    }
    if let Some(file) = file {
        latest.push((file.updated_at, 1_u8, LatestCandidate::File { file }));
    }
    match latest
        .into_iter()
        .max_by_key(|(updated_at, priority, _)| (*updated_at, *priority))
    {
        Some((_, _, LatestCandidate::Url { url })) => Ok(Json(Latest::Url { url })),
        Some((_, _, LatestCandidate::Memo { memo })) => Ok(Json(Latest::Memo { memo })),
        Some((_, _, LatestCandidate::File { file })) => {
            let upload_id = state
                .repository()
                .get_file_upload_id(&actor.user_id, &file.id)
                .await
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::internal("latest file upload is missing"))?;
            let files = state
                .repository()
                .get_files_in_upload(&actor.user_id, &upload_id, now.naive_utc())
                .await
                .map_err(ApiError::internal)?;
            Ok(Json(Latest::File { files }))
        }
        None => Err(ApiError::new(
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
    File { files: Vec<SharedFile> },
}

enum LatestCandidate {
    Url { url: SharedUrl },
    Memo { memo: SharedMemo },
    File { file: SharedFile },
}

struct LatestTypes {
    file: bool,
    url: bool,
    memo: bool,
}

impl LatestTypes {
    fn parse(value: &str) -> Result<Self, ApiError> {
        let mut result = Self {
            file: false,
            url: false,
            memo: false,
        };
        for item in value.bytes() {
            let slot = match item {
                b'f' => &mut result.file,
                b'u' => &mut result.url,
                b'm' => &mut result.memo,
                _ => {
                    return Err(ApiError::bad_request(
                        "INVALID_LATEST_TYPES",
                        "types must contain only f, u, and m",
                    ));
                }
            };
            if *slot {
                return Err(ApiError::bad_request(
                    "INVALID_LATEST_TYPES",
                    "types must not contain duplicates",
                ));
            }
            *slot = true;
        }
        if !(result.file || result.url || result.memo) {
            return Err(ApiError::bad_request("INVALID_LATEST_TYPES", "types must not be empty"));
        }
        Ok(result)
    }
}
