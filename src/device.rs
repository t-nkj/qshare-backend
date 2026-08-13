use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{patch, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    app::{AppState, authenticate, json_body, require_uuid},
    error::ApiError,
    model::Device,
    repository::CreateDevice,
    validation,
};

const TOKEN_PREFIX: &str = "qsh_";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/devices", post(register_device).get(list_devices))
        .route("/v1/devices/{device_id}", patch(rename_device).delete(delete_device))
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
        .repository()
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
        .repository()
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
        .repository()
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
        .repository()
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
