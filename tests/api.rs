use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::{DateTime, NaiveDateTime, Utc};
use qshare_backend::{
    app::{AppState, create_app},
    model::{AuthenticatedDevice, Device, SharedUrl, UrlCursor},
    repository::{CreateDevice, CreateUrl, Repository},
};
use serde_json::{Value, json};
use tower::ServiceExt;

#[derive(Clone)]
struct StoredDevice {
    user_id: String,
    token_hash: Vec<u8>,
    device: Device,
}

#[derive(Clone)]
struct StoredUrl {
    user_id: String,
    url: SharedUrl,
}

#[derive(Default)]
struct MemoryRepository {
    devices: Mutex<Vec<StoredDevice>>,
    urls: Mutex<Vec<StoredUrl>>,
}

#[async_trait]
impl Repository for MemoryRepository {
    async fn create_device(&self, input: CreateDevice<'_>) -> sqlx::Result<Device> {
        let device = Device {
            id: input.id.to_owned(),
            name: input.name.to_owned(),
            created_at: input.now,
            updated_at: input.now,
            last_used_at: None,
        };
        self.devices.lock().unwrap().push(StoredDevice {
            user_id: input.user_id.to_owned(),
            token_hash: input.token_hash.to_vec(),
            device: device.clone(),
        });
        Ok(device)
    }

    async fn find_device_by_token_hash(
        &self,
        token_hash: &[u8],
        now: NaiveDateTime,
    ) -> sqlx::Result<Option<AuthenticatedDevice>> {
        let mut devices = self.devices.lock().unwrap();
        let Some(stored) = devices.iter_mut().find(|device| device.token_hash == token_hash) else {
            return Ok(None);
        };
        stored.device.last_used_at = Some(now);
        Ok(Some(AuthenticatedDevice {
            id: stored.device.id.clone(),
            user_id: stored.user_id.clone(),
            name: stored.device.name.clone(),
        }))
    }

    async fn list_devices(&self, user_id: &str) -> sqlx::Result<Vec<Device>> {
        Ok(self
            .devices
            .lock()
            .unwrap()
            .iter()
            .filter(|device| device.user_id == user_id)
            .map(|device| device.device.clone())
            .collect())
    }

    async fn rename_device(&self, user_id: &str, id: &str, name: &str) -> sqlx::Result<Option<Device>> {
        let mut devices = self.devices.lock().unwrap();
        let Some(stored) = devices
            .iter_mut()
            .find(|device| device.user_id == user_id && device.device.id == id)
        else {
            return Ok(None);
        };
        stored.device.name = name.to_owned();
        stored.device.updated_at += chrono::Duration::milliseconds(1);
        Ok(Some(stored.device.clone()))
    }

    async fn delete_device(&self, user_id: &str, id: &str) -> sqlx::Result<bool> {
        let mut devices = self.devices.lock().unwrap();
        let Some(index) = devices
            .iter()
            .position(|device| device.user_id == user_id && device.device.id == id)
        else {
            return Ok(false);
        };
        devices.remove(index);
        for stored in self.urls.lock().unwrap().iter_mut() {
            if stored.url.source_device_id.as_deref() == Some(id) {
                stored.url.source_device_id = None;
            }
        }
        Ok(true)
    }

    async fn create_url(&self, input: CreateUrl<'_>) -> sqlx::Result<SharedUrl> {
        let url = SharedUrl {
            id: input.id.to_owned(),
            url: input.url.to_owned(),
            source_device_id: Some(input.source_device_id.to_owned()),
            source_device_name: input.source_device_name.to_owned(),
            created_at: input.now,
            expires_at: input.expires_at,
        };
        self.urls.lock().unwrap().push(StoredUrl {
            user_id: input.user_id.to_owned(),
            url: url.clone(),
        });
        Ok(url)
    }

    async fn get_latest_url(&self, user_id: &str, now: NaiveDateTime) -> sqlx::Result<Option<SharedUrl>> {
        Ok(self
            .urls
            .lock()
            .unwrap()
            .iter()
            .filter(|url| url.user_id == user_id && url.url.expires_at > now)
            .max_by_key(|url| (url.url.created_at, url.url.id.clone()))
            .map(|url| url.url.clone()))
    }

    async fn list_urls(
        &self,
        user_id: &str,
        now: NaiveDateTime,
        limit: u32,
        cursor: Option<&UrlCursor>,
    ) -> sqlx::Result<Vec<SharedUrl>> {
        let mut urls: Vec<_> = self
            .urls
            .lock()
            .unwrap()
            .iter()
            .filter(|url| url.user_id == user_id && url.url.expires_at > now)
            .filter(|url| {
                cursor.is_none_or(|cursor| {
                    url.url.created_at < cursor.created_at.naive_utc()
                        || (url.url.created_at == cursor.created_at.naive_utc() && url.url.id < cursor.id)
                })
            })
            .map(|url| url.url.clone())
            .collect();
        urls.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        urls.truncate(limit as usize + 1);
        Ok(urls)
    }

    async fn delete_url(&self, user_id: &str, id: &str) -> sqlx::Result<bool> {
        let mut urls = self.urls.lock().unwrap();
        let Some(index) = urls.iter().position(|url| url.user_id == user_id && url.url.id == id) else {
            return Ok(false);
        };
        urls.remove(index);
        Ok(true)
    }

    async fn delete_expired_urls(&self, now: NaiveDateTime) -> sqlx::Result<u64> {
        let mut urls = self.urls.lock().unwrap();
        let before = urls.len();
        urls.retain(|url| url.url.expires_at > now);
        Ok((before - urls.len()) as u64)
    }
}

fn test_app(repository: Arc<MemoryRepository>, origins: Vec<String>) -> axum::Router {
    let now = DateTime::parse_from_rfc3339("2026-08-12T00:00:00.000Z")
        .unwrap()
        .with_timezone(&Utc);
    create_app(AppState::new(repository, origins).with_clock(move || now))
}

async fn json_response(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

async fn register(app: &axum::Router, user: &str, name: &str) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::post("/v1/devices")
                .header("content-type", "application/json")
                .header("x-forwarded-user", user)
                .body(Body::from(json!({ "name": name }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    json_response(response).await
}

#[tokio::test]
async fn registration_requires_traq_and_token_authenticates() {
    let repository = Arc::new(MemoryRepository::default());
    let app = test_app(repository.clone(), Vec::new());
    let denied = app
        .clone()
        .oneshot(
            Request::post("/v1/devices")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"iPhone"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json_response(denied).await["error"]["code"], "TRAQ_AUTH_REQUIRED");

    let created = register(&app, "alice", " iPhone ").await;
    let token = created["token"].as_str().unwrap();
    assert!(token.starts_with("qsh_"));
    assert_eq!(token.len(), 47);
    assert_eq!(created["device"]["name"], "iPhone");
    assert_eq!(repository.devices.lock().unwrap()[0].token_hash.len(), 32);

    let listed = app
        .oneshot(
            Request::get("/v1/devices")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    assert_eq!(json_response(listed).await["devices"][0]["name"], "iPhone");
}

#[tokio::test]
async fn shares_only_http_urls_and_returns_latest() {
    let app = test_app(Arc::new(MemoryRepository::default()), Vec::new());
    let created = register(&app, "alice", "iPhone").await;
    let token = created["token"].as_str().unwrap();
    let invalid = app
        .clone()
        .oneshot(
            Request::post("/v1/urls")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"url":"javascript:alert(1)"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    let shared = app
        .clone()
        .oneshot(
            Request::post("/v1/urls")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"url":"https://example.com/a?b=1#c"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(shared.status(), StatusCode::CREATED);
    let shared = json_response(shared).await;
    assert_eq!(shared["url"]["sourceDeviceName"], "iPhone");
    assert_eq!(shared["url"]["expiresAt"], "2026-08-19T00:00:00.000Z");

    let latest = app
        .oneshot(
            Request::get("/v1/urls/latest")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(latest.status(), StatusCode::OK);
    assert_eq!(json_response(latest).await["url"]["url"], "https://example.com/a?b=1#c");
}

#[tokio::test]
async fn cors_allows_only_configured_token_clients() {
    let app = test_app(
        Arc::new(MemoryRepository::default()),
        vec!["chrome-extension://allowed".to_owned()],
    );
    let allowed = app
        .clone()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/v1/urls")
                .header("origin", "chrome-extension://allowed")
                .header("access-control-request-method", "POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        allowed.headers()["access-control-allow-origin"],
        "chrome-extension://allowed"
    );

    let registration = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/v1/devices")
                .header("origin", "chrome-extension://allowed")
                .header("access-control-request-method", "POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(registration.status(), StatusCode::FORBIDDEN);
}
