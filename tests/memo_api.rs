mod support;

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use support::{MemoryRepository, json_response, post_memo, register, test_app, test_app_at};
use tower::ServiceExt;

#[tokio::test]
async fn creates_memos_and_extracts_urls_in_order() {
    let repository = Arc::new(MemoryRepository::default());
    let app = test_app(repository.clone(), Vec::new());
    let device = register(&app, "alice", "iPhone").await;
    let token = device["token"].as_str().unwrap();

    let url_only = post_memo(
        &app,
        token,
        json!({
            "content": " \n https://example.com/only \t ",
            "autoDetectUrls": true
        }),
    )
    .await;
    assert_eq!(url_only.status(), StatusCode::CREATED);
    let url_only = json_response(url_only).await;
    assert_eq!(url_only["created"].as_array().unwrap().len(), 1);
    assert_eq!(url_only["created"][0]["type"], "url");
    assert_eq!(repository.memo_count(), 0);

    let mixed_content = "first https://example.com/one. [two](https://example.com/two), again https://example.com/one.";
    let mixed = post_memo(&app, token, json!({ "content": mixed_content, "autoDetectUrls": true })).await;
    assert_eq!(mixed.status(), StatusCode::CREATED);
    let mixed = json_response(mixed).await;
    assert_eq!(mixed["created"].as_array().unwrap().len(), 3);
    assert_eq!(mixed["created"][0]["type"], "url");
    assert_eq!(mixed["created"][0]["url"]["url"], "https://example.com/one");
    assert_eq!(mixed["created"][1]["url"]["url"], "https://example.com/two");
    assert_eq!(mixed["created"][2]["type"], "memo");
    assert_eq!(mixed["created"][2]["memo"]["content"], mixed_content);

    let disabled = post_memo(
        &app,
        token,
        json!({ "content": "https://example.com/not-extracted", "autoDetectUrls": false }),
    )
    .await;
    let disabled = json_response(disabled).await;
    assert_eq!(disabled["created"].as_array().unwrap().len(), 1);
    assert_eq!(disabled["created"][0]["type"], "memo");
}

#[tokio::test]
async fn lists_updates_and_deletes_owner_memos() {
    let repository = Arc::new(MemoryRepository::default());
    let initial = test_app(repository.clone(), Vec::new());
    let alice = register(&initial, "alice", "iPhone").await;
    let bob = register(&initial, "bob", "iPad").await;
    let alice_token = alice["token"].as_str().unwrap();
    let bob_token = bob["token"].as_str().unwrap();
    let created = json_response(post_memo(&initial, alice_token, json!({ "content": "first" })).await).await;
    let memo_id = created["created"][0]["memo"]["id"].as_str().unwrap();

    let later = test_app_at(repository.clone(), Vec::new(), "2026-08-12T01:00:00.000Z");
    let updated = later
        .clone()
        .oneshot(
            Request::patch(format!("/v1/memos/{memo_id}"))
                .header("authorization", format!("Bearer {alice_token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"content":"edited https://example.com/no-auto"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(updated.status(), StatusCode::OK);
    let updated = json_response(updated).await;
    assert_eq!(updated["memo"]["content"], "edited https://example.com/no-auto");
    assert_eq!(updated["memo"]["expiresAt"], "2026-08-19T01:00:00.000Z");
    assert_eq!(repository.url_count(), 0);

    let latest = later
        .clone()
        .oneshot(
            Request::get("/v1/latest/m")
                .header("authorization", format!("Bearer {alice_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(latest.status(), StatusCode::OK);
    let latest = json_response(latest).await;
    assert_eq!(latest["type"], "memo");
    assert_eq!(latest["memo"]["id"], memo_id);

    let forbidden_delete = later
        .clone()
        .oneshot(
            Request::delete(format!("/v1/memos/{memo_id}"))
                .header("authorization", format!("Bearer {bob_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden_delete.status(), StatusCode::NOT_FOUND);

    let deleted = later
        .oneshot(
            Request::delete(format!("/v1/memos/{memo_id}"))
                .header("authorization", format!("Bearer {alice_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn paginates_and_expires_memos() {
    let repository = Arc::new(MemoryRepository::default());
    let first_app = test_app_at(repository.clone(), Vec::new(), "2026-08-12T00:00:00.000Z");
    let device = register(&first_app, "alice", "iPhone").await;
    let token = device["token"].as_str().unwrap();
    assert_eq!(
        post_memo(&first_app, token, json!({ "content": "first" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    let second_app = test_app_at(repository.clone(), Vec::new(), "2026-08-12T00:01:00.000Z");
    assert_eq!(
        post_memo(&second_app, token, json!({ "content": "second" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    let list = second_app
        .clone()
        .oneshot(
            Request::get("/v1/memos?limit=1")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let list = json_response(list).await;
    assert_eq!(list["memos"].as_array().unwrap().len(), 1);
    assert_eq!(list["memos"][0]["content"], "second");
    let cursor = list["nextCursor"].as_str().unwrap();

    let next = second_app
        .clone()
        .oneshot(
            Request::get(format!("/v1/memos?limit=1&cursor={cursor}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_response(next).await["memos"][0]["content"], "first");

    let expired_app = test_app_at(repository, Vec::new(), "2026-08-19T00:01:00.001Z");
    let expired = expired_app
        .oneshot(
            Request::get("/v1/latest/m")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(expired.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_response(expired).await["error"]["code"], "LATEST_NOT_FOUND");
}
