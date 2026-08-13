#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::{DateTime, NaiveDateTime, Utc};
use qshare_backend::{
    app::{AppState, create_app},
    model::{AuthenticatedDevice, Device, SharedFile, SharedMemo, SharedUrl, UrlCursor},
    repository::{
        CreateDevice, CreateFile, CreateMemoBundle, CreateUrl, CreatedFile, CreatedMemoItem, FileRecord, Repository,
    },
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

#[derive(Clone)]
struct StoredMemo {
    user_id: String,
    memo: SharedMemo,
}

#[derive(Clone)]
struct StoredFile {
    user_id: String,
    upload_id: String,
    storage_key: String,
    file: SharedFile,
}

#[derive(Default)]
pub struct MemoryRepository {
    devices: Mutex<Vec<StoredDevice>>,
    urls: Mutex<Vec<StoredUrl>>,
    memos: Mutex<Vec<StoredMemo>>,
    files: Mutex<Vec<StoredFile>>,
}

impl MemoryRepository {
    pub fn memo_count(&self) -> usize {
        self.memos.lock().unwrap().len()
    }

    pub fn url_count(&self) -> usize {
        self.urls.lock().unwrap().len()
    }
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
        for stored in self.memos.lock().unwrap().iter_mut() {
            if stored.memo.source_device_id.as_deref() == Some(id) {
                stored.memo.source_device_id = None;
            }
        }
        for stored in self.files.lock().unwrap().iter_mut() {
            if stored.file.source_device_id.as_deref() == Some(id) {
                stored.file.source_device_id = None;
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
        sort_newest(&mut urls, |url| (url.created_at, url.id.clone()));
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

    async fn create_memo_bundle(&self, input: CreateMemoBundle) -> sqlx::Result<Vec<CreatedMemoItem>> {
        let mut created = Vec::new();
        for input in input.urls {
            let url = SharedUrl {
                id: input.id,
                url: input.url,
                source_device_id: Some(input.source_device_id),
                source_device_name: input.source_device_name,
                created_at: input.now,
                expires_at: input.expires_at,
            };
            self.urls.lock().unwrap().push(StoredUrl {
                user_id: input.user_id,
                url: url.clone(),
            });
            created.push(CreatedMemoItem::Url(url));
        }
        if let Some(input) = input.memo {
            let memo = SharedMemo {
                id: input.id,
                content: input.content,
                source_device_id: Some(input.source_device_id),
                source_device_name: input.source_device_name,
                created_at: input.now,
                updated_at: input.now,
                expires_at: input.expires_at,
            };
            self.memos.lock().unwrap().push(StoredMemo {
                user_id: input.user_id,
                memo: memo.clone(),
            });
            created.push(CreatedMemoItem::Memo(memo));
        }
        Ok(created)
    }

    async fn get_latest_memo(&self, user_id: &str, now: NaiveDateTime) -> sqlx::Result<Option<SharedMemo>> {
        Ok(self
            .memos
            .lock()
            .unwrap()
            .iter()
            .filter(|memo| memo.user_id == user_id && memo.memo.expires_at > now)
            .max_by_key(|memo| (memo.memo.updated_at, memo.memo.id.clone()))
            .map(|memo| memo.memo.clone()))
    }

    async fn list_memos(
        &self,
        user_id: &str,
        now: NaiveDateTime,
        limit: u32,
        cursor: Option<&UrlCursor>,
    ) -> sqlx::Result<Vec<SharedMemo>> {
        let mut memos: Vec<_> = self
            .memos
            .lock()
            .unwrap()
            .iter()
            .filter(|memo| memo.user_id == user_id && memo.memo.expires_at > now)
            .filter(|memo| {
                cursor.is_none_or(|cursor| {
                    memo.memo.created_at < cursor.created_at.naive_utc()
                        || (memo.memo.created_at == cursor.created_at.naive_utc() && memo.memo.id < cursor.id)
                })
            })
            .map(|memo| memo.memo.clone())
            .collect();
        sort_newest(&mut memos, |memo| (memo.created_at, memo.id.clone()));
        memos.truncate(limit as usize + 1);
        Ok(memos)
    }

    async fn update_memo(
        &self,
        user_id: &str,
        id: &str,
        content: &str,
        now: NaiveDateTime,
        expires_at: NaiveDateTime,
    ) -> sqlx::Result<Option<SharedMemo>> {
        let mut memos = self.memos.lock().unwrap();
        let Some(stored) = memos
            .iter_mut()
            .find(|memo| memo.user_id == user_id && memo.memo.id == id)
        else {
            return Ok(None);
        };
        stored.memo.content = content.to_owned();
        stored.memo.updated_at = now;
        stored.memo.expires_at = expires_at;
        Ok(Some(stored.memo.clone()))
    }

    async fn delete_memo(&self, user_id: &str, id: &str) -> sqlx::Result<bool> {
        let mut memos = self.memos.lock().unwrap();
        let Some(index) = memos
            .iter()
            .position(|memo| memo.user_id == user_id && memo.memo.id == id)
        else {
            return Ok(false);
        };
        memos.remove(index);
        Ok(true)
    }

    async fn delete_expired_memos(&self, now: NaiveDateTime) -> sqlx::Result<u64> {
        let mut memos = self.memos.lock().unwrap();
        let before = memos.len();
        memos.retain(|memo| memo.memo.expires_at > now);
        Ok((before - memos.len()) as u64)
    }

    async fn create_file_and_evict_once(&self, input: CreateFile, maximum_bytes: u64) -> sqlx::Result<CreatedFile> {
        let file = SharedFile {
            id: input.id,
            name: input.name,
            content_type: input.content_type,
            size: input.size,
            source_device_id: Some(input.source_device_id),
            source_device_name: input.source_device_name,
            created_at: input.now,
            updated_at: input.now,
            expires_at: input.expires_at,
        };
        let mut files = self.files.lock().unwrap();
        files.push(StoredFile {
            user_id: input.user_id.clone(),
            upload_id: input.upload_id,
            storage_key: input.storage_key,
            file: file.clone(),
        });
        let mut indexes: Vec<_> = files
            .iter()
            .enumerate()
            .filter(|(_, item)| item.user_id == input.user_id)
            .map(|(index, _)| index)
            .collect();
        indexes.sort_by_key(|index| {
            (
                files[*index].file.updated_at,
                files[*index].file.id.clone() == file.id,
                files[*index].file.id.clone(),
            )
        });
        let mut total: u64 = indexes.iter().map(|index| files[*index].file.size).sum();
        let mut evicted_ids = Vec::new();
        for index in indexes {
            if total <= maximum_bytes {
                break;
            }
            total -= files[index].file.size;
            evicted_ids.push(files[index].file.id.clone());
        }
        let mut evicted = Vec::new();
        files.retain(|stored| {
            if evicted_ids.contains(&stored.file.id) {
                evicted.push(FileRecord {
                    file: stored.file.clone(),
                    storage_key: stored.storage_key.clone(),
                });
                false
            } else {
                true
            }
        });
        Ok(CreatedFile { file, evicted })
    }

    async fn get_file(&self, user_id: &str, id: &str, now: NaiveDateTime) -> sqlx::Result<Option<FileRecord>> {
        Ok(self
            .files
            .lock()
            .unwrap()
            .iter()
            .find(|item| item.user_id == user_id && item.file.id == id && item.file.expires_at > now)
            .map(|item| FileRecord {
                file: item.file.clone(),
                storage_key: item.storage_key.clone(),
            }))
    }

    async fn get_latest_file(&self, user_id: &str, now: NaiveDateTime) -> sqlx::Result<Option<SharedFile>> {
        Ok(self
            .files
            .lock()
            .unwrap()
            .iter()
            .filter(|item| item.user_id == user_id && item.file.expires_at > now)
            .max_by_key(|item| (item.file.updated_at, item.file.id.clone()))
            .map(|item| item.file.clone()))
    }

    async fn get_file_upload_id(&self, user_id: &str, id: &str) -> sqlx::Result<Option<String>> {
        Ok(self
            .files
            .lock()
            .unwrap()
            .iter()
            .find(|item| item.user_id == user_id && item.file.id == id)
            .map(|item| item.upload_id.clone()))
    }

    async fn get_files_in_upload(
        &self,
        user_id: &str,
        upload_id: &str,
        now: NaiveDateTime,
    ) -> sqlx::Result<Vec<SharedFile>> {
        let mut files: Vec<_> = self
            .files
            .lock()
            .unwrap()
            .iter()
            .filter(|item| item.user_id == user_id && item.upload_id == upload_id && item.file.expires_at > now)
            .map(|item| item.file.clone())
            .collect();
        files.sort_by_key(|file| (file.created_at, file.id.clone()));
        Ok(files)
    }

    async fn list_files(
        &self,
        user_id: &str,
        now: NaiveDateTime,
        limit: u32,
        cursor: Option<&UrlCursor>,
    ) -> sqlx::Result<Vec<SharedFile>> {
        let mut files: Vec<_> = self
            .files
            .lock()
            .unwrap()
            .iter()
            .filter(|item| item.user_id == user_id && item.file.expires_at > now)
            .filter(|item| {
                cursor.is_none_or(|cursor| {
                    item.file.created_at < cursor.created_at.naive_utc()
                        || (item.file.created_at == cursor.created_at.naive_utc() && item.file.id < cursor.id)
                })
            })
            .map(|item| item.file.clone())
            .collect();
        sort_newest(&mut files, |file| (file.created_at, file.id.clone()));
        files.truncate(limit as usize + 1);
        Ok(files)
    }

    async fn rename_file(
        &self,
        user_id: &str,
        id: &str,
        name: &str,
        now: NaiveDateTime,
        expires_at: NaiveDateTime,
    ) -> sqlx::Result<Option<SharedFile>> {
        let mut files = self.files.lock().unwrap();
        let Some(file) = files
            .iter_mut()
            .find(|item| item.user_id == user_id && item.file.id == id)
        else {
            return Ok(None);
        };
        file.file.name = name.to_owned();
        file.file.updated_at = now;
        file.file.expires_at = expires_at;
        Ok(Some(file.file.clone()))
    }

    async fn delete_file(&self, user_id: &str, id: &str) -> sqlx::Result<Option<FileRecord>> {
        let mut files = self.files.lock().unwrap();
        let Some(index) = files
            .iter()
            .position(|item| item.user_id == user_id && item.file.id == id)
        else {
            return Ok(None);
        };
        let file = files.remove(index);
        Ok(Some(FileRecord {
            file: file.file,
            storage_key: file.storage_key,
        }))
    }

    async fn delete_expired_files(&self, now: NaiveDateTime) -> sqlx::Result<Vec<FileRecord>> {
        let mut files = self.files.lock().unwrap();
        let mut deleted = Vec::new();
        files.retain(|item| {
            if item.file.expires_at <= now {
                deleted.push(FileRecord {
                    file: item.file.clone(),
                    storage_key: item.storage_key.clone(),
                });
                false
            } else {
                true
            }
        });
        Ok(deleted)
    }

    async fn clear_files(&self) -> sqlx::Result<()> {
        self.files.lock().unwrap().clear();
        Ok(())
    }
}

fn sort_newest<T>(items: &mut [T], key: impl Fn(&T) -> (NaiveDateTime, String)) {
    items.sort_by_key(|item| std::cmp::Reverse(key(item)));
}

pub fn test_app(repository: Arc<MemoryRepository>, origins: Vec<String>) -> axum::Router {
    test_app_at(repository, origins, "2026-08-12T00:00:00.000Z")
}

pub fn test_app_at(repository: Arc<MemoryRepository>, origins: Vec<String>, now: &str) -> axum::Router {
    let now = DateTime::parse_from_rfc3339(now).unwrap().with_timezone(&Utc);
    create_app(AppState::new(repository, origins).with_clock(move || now))
}

pub async fn json_response(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

pub async fn register(app: &axum::Router, user: &str, name: &str) -> Value {
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

pub async fn post_memo(app: &axum::Router, token: &str, body: Value) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::post("/v1/memos")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
