mod support;

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use support::{MemoryRepository, json_response, register, test_app};
use tower::ServiceExt;

#[tokio::test]
async fn shares_only_http_urls_and_returns_latest_url() {
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
            Request::get("/v1/latest/u")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(latest.status(), StatusCode::OK);
    let latest = json_response(latest).await;
    assert_eq!(latest["type"], "url");
    assert_eq!(latest["url"]["url"], "https://example.com/a?b=1#c");
}
