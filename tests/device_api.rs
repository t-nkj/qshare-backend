mod support;

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use support::{MemoryRepository, json_response, register, test_app};
use tower::ServiceExt;

#[tokio::test]
async fn registration_requires_traq_and_token_authenticates() {
    let repository = Arc::new(MemoryRepository::default());
    let app = test_app(repository, Vec::new());
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
