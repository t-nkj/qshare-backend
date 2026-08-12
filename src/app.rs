use std::{collections::HashSet, sync::Arc};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};
use uuid::Uuid;

use crate::{
    error::ApiError,
    model::{AuthenticatedDevice, Device, SharedUrl},
    repository::{CreateDevice, CreateUrl, Repository},
    validation,
};

const JSON_BODY_LIMIT: usize = 16 * 1024;
const TOKEN_PREFIX: &str = "qsh_";

#[derive(Clone)]
pub struct AppState {
    repository: Arc<dyn Repository>,
    allowed_origins: Arc<HashSet<String>>,
    clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

impl AppState {
    pub fn new(repository: Arc<dyn Repository>, cors_allowed_origins: Vec<String>) -> Self {
        Self {
            repository,
            allowed_origins: Arc::new(cors_allowed_origins.into_iter().collect()),
            clock: Arc::new(Utc::now),
        }
    }

    #[doc(hidden)]
    pub fn with_clock(mut self, clock: impl Fn() -> DateTime<Utc> + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    fn now(&self) -> DateTime<Utc> {
        (self.clock)()
    }
}

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/v1/devices", post(register_device).get(list_devices))
        .route("/v1/devices/{device_id}", patch(rename_device).delete(delete_device))
        .route("/v1/urls", post(create_url).get(list_urls))
        .route("/v1/urls/latest", get(latest_url))
        .route("/v1/urls/{url_id}", delete(delete_url))
        .fallback(not_found)
        .layer(RequestBodyLimitLayer::new(JSON_BODY_LIMIT))
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(state.clone(), response_middleware))
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn register_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = forwarded_user(&headers)?;
    let body = json_body(&headers, &body)?;
    let name = validation::device_name(&body)?;
    let now = state.now().naive_utc();
    let mut random = [0_u8; 32];
    rand::rng().fill_bytes(&mut random);
    let token = format!("{TOKEN_PREFIX}{}", URL_SAFE_NO_PAD.encode(random));
    let token_hash = Sha256::digest(token.as_bytes());
    let id = Uuid::new_v4().to_string();
    let device = state
        .repository
        .create_device(CreateDevice {
            id: &id,
            user_id,
            name: &name,
            token_hash: &token_hash,
            now,
        })
        .await
        .map_err(ApiError::internal)?;
    Ok((StatusCode::CREATED, Json(DeviceCreated { device, token })))
}

async fn list_devices(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Devices>, ApiError> {
    let actor = authenticate(&state, &headers).await?;
    let devices = state
        .repository
        .list_devices(&actor.user_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(Devices { devices }))
}

async fn rename_device(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<DeviceEnvelope>, ApiError> {
    let actor = authenticate(&state, &headers).await?;
    require_uuid(&device_id, "DEVICE_NOT_FOUND", "device was not found")?;
    let name = validation::device_name(&json_body(&headers, &body)?)?;
    let device = state
        .repository
        .rename_device(&actor.user_id, &device_id, &name)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "DEVICE_NOT_FOUND", "device was not found"))?;
    Ok(Json(DeviceEnvelope { device }))
}

async fn delete_device(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let actor = authenticate(&state, &headers).await?;
    require_uuid(&device_id, "DEVICE_NOT_FOUND", "device was not found")?;
    let deleted = state
        .repository
        .delete_device(&actor.user_id, &device_id)
        .await
        .map_err(ApiError::internal)?;
    if !deleted {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "DEVICE_NOT_FOUND",
            "device was not found",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
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
        .repository
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

async fn latest_url(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<UrlEnvelope>, ApiError> {
    let now = state.now();
    let actor = authenticate_at(&state, &headers, now).await?;
    let url = state
        .repository
        .get_latest_url(&actor.user_id, now.naive_utc())
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "URL_NOT_FOUND", "no unexpired URL was found"))?;
    Ok(Json(UrlEnvelope { url }))
}

#[derive(serde::Deserialize)]
struct ListQuery {
    limit: Option<String>,
    cursor: Option<String>,
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
        .repository
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
        .repository
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

async fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<AuthenticatedDevice, ApiError> {
    authenticate_at(state, headers, state.now()).await
}

async fn authenticate_at(
    state: &AppState,
    headers: &HeaderMap,
    now: DateTime<Utc>,
) -> Result<AuthenticatedDevice, ApiError> {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|token| token.starts_with(TOKEN_PREFIX) && token.len() >= 20)
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

fn forwarded_user(headers: &HeaderMap) -> Result<&str, ApiError> {
    let value = headers
        .get("x-forwarded-user")
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|character| character.is_ascii_alphanumeric() || matches!(character, b'_' | b'-'))
        });
    value.ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "TRAQ_AUTH_REQUIRED",
            "traQ authentication is required",
        )
    })
}

fn json_body(headers: &HeaderMap, body: &[u8]) -> Result<Value, ApiError> {
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

fn require_uuid(value: &str, code: &'static str, message: &'static str) -> Result<(), ApiError> {
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

#[derive(Serialize)]
struct DeviceCreated {
    device: Device,
    token: String,
}

#[derive(Serialize)]
struct Devices {
    devices: Vec<Device>,
}

#[derive(Serialize)]
struct DeviceEnvelope {
    device: Device,
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
