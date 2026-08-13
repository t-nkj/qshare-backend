mod support;

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use serde_json::json;
use tower::ServiceExt;

use support::{MemoryRepository, json_response, post_memo, register, test_app};

#[tokio::test]
async fn returns_the_latest_url_or_memo() {
    let repository = Arc::new(MemoryRepository::default());
    let app = test_app(repository, vec![]);
    let registered = register(&app, "alice", "iPhone").await;
    let token = registered["token"].as_str().unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/urls")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "url": "https://example.com/" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/latest/u")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_response(response).await["type"], "url");

    let response = post_memo(&app, token, json!({ "content": "newer memo" })).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/latest/um")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = json_response(response).await;
    assert_eq!(body["type"], "memo");
    assert_eq!(body["memo"]["content"], "newer memo");
}

#[tokio::test]
async fn returns_the_latest_url_or_memo_from_the_combined_types_endpoint() {
    let repository = Arc::new(MemoryRepository::default());
    let app = test_app(repository, vec![]);
    let registered = register(&app, "alice", "iPhone").await;
    let token = registered["token"].as_str().unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/urls")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "url": "https://example.com/" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/latest/mu")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_response(response).await["type"], "url");
}

#[tokio::test]
async fn reports_not_found_when_no_content_exists() {
    let repository = Arc::new(MemoryRepository::default());
    let app = test_app(repository, vec![]);
    let registered = register(&app, "alice", "iPhone").await;
    let token = registered["token"].as_str().unwrap();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/latest/fum")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_response(response).await["error"]["code"], "LATEST_NOT_FOUND");
}
