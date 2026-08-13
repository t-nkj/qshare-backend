mod support;

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use support::{MemoryRepository, test_app};
use tower::ServiceExt;

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
