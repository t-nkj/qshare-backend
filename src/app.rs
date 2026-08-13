use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::{error::ApiError, model::AuthenticatedDevice, repository::Repository};

const JSON_BODY_LIMIT: usize = 16 * 1024;

#[derive(Clone)]
pub struct AppState {
    repository: Arc<dyn Repository>,
    allowed_origins: Arc<HashSet<String>>,
    clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    file_storage_dir: Arc<PathBuf>,
    thumbnail_generation: Arc<Semaphore>,
}

impl AppState {
    pub fn new(repository: Arc<dyn Repository>, cors_allowed_origins: Vec<String>) -> Self {
        Self {
            repository,
            allowed_origins: Arc::new(cors_allowed_origins.into_iter().collect()),
            clock: Arc::new(Utc::now),
            file_storage_dir: Arc::new(PathBuf::from("/tmp/qshare-files")),
            thumbnail_generation: Arc::new(Semaphore::new(1)),
        }
    }

    #[doc(hidden)]
    pub fn with_clock(mut self, clock: impl Fn() -> DateTime<Utc> + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    pub(crate) fn now(&self) -> DateTime<Utc> {
        (self.clock)()
    }

    pub(crate) fn repository(&self) -> &Arc<dyn Repository> {
        &self.repository
    }

    pub fn with_file_storage_dir(mut self, directory: PathBuf) -> Self {
        self.file_storage_dir = Arc::new(directory);
        self
    }

    pub(crate) fn file_storage_dir(&self) -> &Path {
        self.file_storage_dir.as_ref()
    }

    pub(crate) async fn acquire_thumbnail_generation(&self) -> OwnedSemaphorePermit {
        self.thumbnail_generation
            .clone()
            .acquire_owned()
            .await
            .expect("thumbnail generation semaphore is never closed")
    }
}

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .merge(crate::device::routes())
        .merge(crate::url::routes())
        .merge(crate::memo::routes())
        .merge(crate::file::routes())
        .merge(crate::latest::routes())
        .fallback(not_found)
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(state.clone(), response_middleware))
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

#[derive(serde::Deserialize)]
pub(crate) struct ListQuery {
    pub(crate) limit: Option<String>,
    pub(crate) cursor: Option<String>,
}

pub(crate) async fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<AuthenticatedDevice, ApiError> {
    authenticate_at(state, headers, state.now()).await
}

pub(crate) async fn authenticate_at(
    state: &AppState,
    headers: &HeaderMap,
    now: DateTime<Utc>,
) -> Result<AuthenticatedDevice, ApiError> {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|token| token.starts_with("qsh_") && token.len() >= 20)
        .ok_or_else(invalid_token)?;
    let hash = Sha256::digest(authorization.as_bytes());
    state
        .repository
        .find_device_by_token_hash(&hash, now.naive_utc())
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(invalid_token)
}

fn invalid_token() -> ApiError {
    ApiError::unauthorized("INVALID_TOKEN", "a valid device token is required")
}

pub(crate) fn json_body(headers: &HeaderMap, body: &[u8]) -> Result<Value, ApiError> {
    if body.len() > JSON_BODY_LIMIT {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "JSON_BODY_TOO_LARGE",
            "JSON request body must not exceed 16 KiB",
        ));
    }
    let is_json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("application/json"));
    if !is_json {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "UNSUPPORTED_MEDIA_TYPE",
            "Content-Type must be application/json",
        ));
    }
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| ApiError::bad_request("INVALID_JSON", "request body must be a JSON object"))?;
    if !value.is_object() {
        return Err(ApiError::bad_request(
            "INVALID_JSON",
            "request body must be a JSON object",
        ));
    }
    Ok(value)
}

pub(crate) fn require_uuid(value: &str, code: &'static str, message: &'static str) -> Result<(), ApiError> {
    let valid = Uuid::parse_str(value).is_ok_and(|id| id.get_version_num() == 4);
    if valid {
        Ok(())
    } else {
        Err(ApiError::new(StatusCode::NOT_FOUND, code, message))
    }
}

async fn response_middleware(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let path = request.uri().path().to_owned();
    let is_api = path == "/v1" || path.starts_with("/v1/");
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let origin_allowed = origin
        .as_ref()
        .is_some_and(|origin| state.allowed_origins.contains(origin));

    if is_api && request.method() == Method::OPTIONS {
        let requested_method = request
            .headers()
            .get(header::ACCESS_CONTROL_REQUEST_METHOD)
            .and_then(|value| value.to_str().ok());
        if path == "/v1/devices" && requested_method == Some("POST") {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "CORS_NOT_ALLOWED",
                "device registration must be same-origin",
            ));
        }
        if !origin_allowed {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "CORS_NOT_ALLOWED",
                "origin is not allowed",
            ));
        }
        let mut response = StatusCode::NO_CONTENT.into_response();
        add_cors_headers(
            &mut response,
            origin.as_deref().expect("allowed origin must exist"),
            true,
        );
        return Ok(response);
    }

    let is_registration = path == "/v1/devices" && request.method() == Method::POST;
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if is_api {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    if origin_allowed && !is_registration {
        add_cors_headers(
            &mut response,
            origin.as_deref().expect("allowed origin must exist"),
            false,
        );
    }
    Ok(response)
}

fn add_cors_headers(response: &mut Response, origin: &str, preflight: bool) {
    let Ok(origin) = HeaderValue::from_str(origin) else {
        return;
    };
    response
        .headers_mut()
        .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Origin"));
    if preflight {
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, PATCH, POST, DELETE, OPTIONS"),
        );
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("Authorization, Content-Type"),
        );
        response
            .headers_mut()
            .insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
    }
}

async fn not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "NOT_FOUND", "endpoint was not found")
}
