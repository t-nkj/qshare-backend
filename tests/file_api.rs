mod support;

use std::{path::PathBuf, sync::Arc};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use image::{ImageBuffer, ImageFormat, Rgb};
use qshare_backend::app::{AppState, create_app};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use support::{MemoryRepository, json_response, register};

fn test_app(repository: Arc<MemoryRepository>, directory: PathBuf) -> axum::Router {
    create_app(AppState::new(repository, vec![]).with_file_storage_dir(directory))
}

fn multipart(files: &[(&str, &str, &[u8])]) -> (String, Vec<u8>) {
    let boundary = "qshare-test-boundary";
    let mut body = Vec::new();
    for (field, name, contents) in files {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{name}\"\r\nContent-Type: text/plain\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(contents);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn jpeg() -> Vec<u8> {
    let image = ImageBuffer::from_pixel(16, 8, Rgb([0_u8, 160, 255]));
    let mut output = std::io::Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Jpeg).unwrap();
    output.into_inner()
}

#[tokio::test]
async fn uploads_multiple_files_then_renames_downloads_and_deletes_a_file() {
    let directory = std::env::temp_dir().join(format!("qshare-file-test-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let app = test_app(Arc::new(MemoryRepository::default()), directory.clone());
    let registered = register(&app, "alice", "iPhone").await;
    let token = registered["token"].as_str().unwrap();
    let (content_type, body) = multipart(&[("files", "hello.txt", b"hello"), ("files", "second.txt", b"second")]);
    let response = app
        .clone()
        .oneshot(
            Request::post("/v1/files")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let response_body = json_response(response).await;
    assert_eq!(status, StatusCode::CREATED, "{response_body}");
    assert_eq!(response_body["failed"], json!([]));
    assert_eq!(response_body["created"].as_array().unwrap().len(), 2);
    let file = response_body["created"][0].clone();
    let second_id = response_body["created"][1]["id"].as_str().unwrap().to_owned();
    assert_eq!(file["name"], "hello.txt");
    assert_eq!(file["size"], 5);
    assert_eq!(file["hasThumbnail"], false);
    let id = file["id"].as_str().unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/files")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let files = json_response(response).await;
    assert_eq!(files["files"].as_array().unwrap().len(), 2);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/files/{id}/thumbnail"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        json_response(response).await["error"]["code"],
        "THUMBNAIL_NOT_AVAILABLE"
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/latest/f")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let latest = json_response(response).await;
    assert_eq!(latest["type"], "file");
    assert_eq!(latest["files"].as_array().unwrap().len(), 2);
    assert_eq!(latest["files"][0]["id"], id);
    assert_eq!(latest["files"][1]["id"], second_id);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/files/{id}"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "name": "renamed.txt" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_response(response).await["file"]["name"], "renamed.txt");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/files/{id}"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename*=UTF-8''renamed.txt"
    );
    assert_eq!(to_bytes(response.into_body(), usize::MAX).await.unwrap(), "hello");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/files/{id}"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/files/{second_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        tokio::fs::read_dir(&directory)
            .await
            .unwrap()
            .next_entry()
            .await
            .unwrap()
            .is_none()
    );
    tokio::fs::remove_dir_all(directory).await.unwrap();
}

#[tokio::test]
async fn keeps_valid_files_when_another_field_is_invalid() {
    let directory = std::env::temp_dir().join(format!("qshare-file-test-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let app = test_app(Arc::new(MemoryRepository::default()), directory.clone());
    let registered = register(&app, "alice", "iPhone").await;
    let token = registered["token"].as_str().unwrap();
    let (content_type, body) = multipart(&[
        ("files", "first.txt", b"first"),
        ("file", "legacy.txt", b"legacy"),
        ("files", "last.txt", b"last"),
    ]);
    let response = app
        .clone()
        .oneshot(
            Request::post("/v1/files")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let response = json_response(response).await;
    assert_eq!(response["created"].as_array().unwrap().len(), 2);
    assert_eq!(response["created"][0]["name"], "first.txt");
    assert_eq!(response["created"][1]["name"], "last.txt");
    assert_eq!(response["failed"][0]["index"], 1);
    assert_eq!(response["failed"][0]["name"], "legacy.txt");
    assert_eq!(response["failed"][0]["error"]["code"], "INVALID_MULTIPART");

    let (content_type, body) = multipart(&[("file", "legacy.txt", b"legacy")]);
    let response = app
        .oneshot(
            Request::post("/v1/files")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_response(response).await["error"]["code"], "INVALID_MULTIPART");
    tokio::fs::remove_dir_all(directory).await.unwrap();
}

#[tokio::test]
async fn creates_and_returns_an_image_thumbnail() {
    let directory = std::env::temp_dir().join(format!("qshare-file-test-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let app = test_app(Arc::new(MemoryRepository::default()), directory.clone());
    let registered = register(&app, "alice", "iPhone").await;
    let token = registered["token"].as_str().unwrap();
    let boundary = "qshare-image-test-boundary";
    let image = jpeg();
    let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"image.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n").into_bytes();
    body.extend_from_slice(&image);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let response = app
        .clone()
        .oneshot(
            Request::post("/v1/files")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let response = json_response(response).await;
    let file = &response["created"][0];
    assert_eq!(file["hasThumbnail"], true);
    let id = file["id"].as_str().unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/files/{id}/thumbnail"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/webp");
    let thumbnail = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(image::load_from_memory_with_format(&thumbnail, ImageFormat::WebP).is_ok());
    tokio::fs::remove_dir_all(directory).await.unwrap();
}
