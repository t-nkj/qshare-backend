use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::{MySqlPool, mysql::MySqlPoolOptions};

use crate::model::{AuthenticatedDevice, Device, SharedFile, SharedMemo, SharedUrl, UrlCursor};

pub struct CreateDevice<'a> {
    pub id: &'a str,
    pub user_id: &'a str,
    pub name: &'a str,
    pub token_hash: &'a [u8],
    pub now: NaiveDateTime,
}

pub struct CreateUrl<'a> {
    pub id: &'a str,
    pub user_id: &'a str,
    pub source_device_id: &'a str,
    pub source_device_name: &'a str,
    pub url: &'a str,
    pub now: NaiveDateTime,
    pub expires_at: NaiveDateTime,
}

pub struct CreateMemo {
    pub id: String,
    pub user_id: String,
    pub source_device_id: String,
    pub source_device_name: String,
    pub content: String,
    pub now: NaiveDateTime,
    pub expires_at: NaiveDateTime,
}

pub struct CreateMemoBundle {
    pub urls: Vec<CreateUrlOwned>,
    pub memo: Option<CreateMemo>,
}

pub struct CreateUrlOwned {
    pub id: String,
    pub user_id: String,
    pub source_device_id: String,
    pub source_device_name: String,
    pub url: String,
    pub now: NaiveDateTime,
    pub expires_at: NaiveDateTime,
}

pub enum CreatedMemoItem {
    Url(SharedUrl),
    Memo(SharedMemo),
}

#[derive(Clone)]
pub struct CreateFile {
    pub id: String,
    pub user_id: String,
    pub source_device_id: String,
    pub source_device_name: String,
    pub name: String,
    pub content_type: String,
    pub size: u64,
    pub storage_key: String,
    pub now: NaiveDateTime,
    pub expires_at: NaiveDateTime,
}

#[derive(Clone)]
pub struct FileRecord {
    pub file: SharedFile,
    pub storage_key: String,
}

pub struct CreatedFile {
    pub file: SharedFile,
    pub evicted: Vec<FileRecord>,
}

#[async_trait]
pub trait Repository: Send + Sync {
    async fn create_device(&self, input: CreateDevice<'_>) -> sqlx::Result<Device>;
    async fn find_device_by_token_hash(
        &self,
        token_hash: &[u8],
        now: NaiveDateTime,
    ) -> sqlx::Result<Option<AuthenticatedDevice>>;
    async fn list_devices(&self, user_id: &str) -> sqlx::Result<Vec<Device>>;
    async fn rename_device(&self, user_id: &str, id: &str, name: &str) -> sqlx::Result<Option<Device>>;
    async fn delete_device(&self, user_id: &str, id: &str) -> sqlx::Result<bool>;
    async fn create_url(&self, input: CreateUrl<'_>) -> sqlx::Result<SharedUrl>;
    async fn get_latest_url(&self, user_id: &str, now: NaiveDateTime) -> sqlx::Result<Option<SharedUrl>>;
    async fn list_urls(
        &self,
        user_id: &str,
        now: NaiveDateTime,
        limit: u32,
        cursor: Option<&UrlCursor>,
    ) -> sqlx::Result<Vec<SharedUrl>>;
    async fn delete_url(&self, user_id: &str, id: &str) -> sqlx::Result<bool>;
    async fn delete_expired_urls(&self, now: NaiveDateTime) -> sqlx::Result<u64>;
    async fn create_memo_bundle(&self, input: CreateMemoBundle) -> sqlx::Result<Vec<CreatedMemoItem>>;
    async fn get_latest_memo(&self, user_id: &str, now: NaiveDateTime) -> sqlx::Result<Option<SharedMemo>>;
    async fn list_memos(
        &self,
        user_id: &str,
        now: NaiveDateTime,
        limit: u32,
        cursor: Option<&UrlCursor>,
    ) -> sqlx::Result<Vec<SharedMemo>>;
    async fn update_memo(
        &self,
        user_id: &str,
        id: &str,
        content: &str,
        now: NaiveDateTime,
        expires_at: NaiveDateTime,
    ) -> sqlx::Result<Option<SharedMemo>>;
    async fn delete_memo(&self, user_id: &str, id: &str) -> sqlx::Result<bool>;
    async fn delete_expired_memos(&self, now: NaiveDateTime) -> sqlx::Result<u64>;
    async fn create_file_and_evict(&self, input: CreateFile, maximum_bytes: u64) -> sqlx::Result<CreatedFile> {
        self.create_file_and_evict_once(input, maximum_bytes).await
    }
    async fn create_file_and_evict_once(&self, input: CreateFile, maximum_bytes: u64) -> sqlx::Result<CreatedFile>;
    async fn get_file(&self, user_id: &str, id: &str, now: NaiveDateTime) -> sqlx::Result<Option<FileRecord>>;
    async fn get_latest_file(&self, user_id: &str, now: NaiveDateTime) -> sqlx::Result<Option<SharedFile>>;
    async fn list_files(
        &self,
        user_id: &str,
        now: NaiveDateTime,
        limit: u32,
        cursor: Option<&UrlCursor>,
    ) -> sqlx::Result<Vec<SharedFile>>;
    async fn rename_file(
        &self,
        user_id: &str,
        id: &str,
        name: &str,
        now: NaiveDateTime,
        expires_at: NaiveDateTime,
    ) -> sqlx::Result<Option<SharedFile>>;
    async fn delete_file(&self, user_id: &str, id: &str) -> sqlx::Result<Option<FileRecord>>;
    async fn delete_expired_files(&self, now: NaiveDateTime) -> sqlx::Result<Vec<FileRecord>>;
    async fn clear_files(&self) -> sqlx::Result<()>;
}

pub struct MySqlRepository {
    pool: MySqlPool,
    file_write_lock: tokio::sync::Mutex<()>,
}

impl MySqlRepository {
    pub async fn connect(database_url: &str) -> sqlx::Result<Self> {
        let pool = MySqlPoolOptions::new()
            .min_connections(0)
            .max_connections(5)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(database_url)
            .await?;
        Ok(Self {
            pool,
            file_write_lock: tokio::sync::Mutex::new(()),
        })
    }

    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!().run(&self.pool).await
    }
}

#[async_trait]
impl Repository for MySqlRepository {
    async fn create_device(&self, input: CreateDevice<'_>) -> sqlx::Result<Device> {
        sqlx::query(
            "INSERT INTO devices (id, user_id, name, token_hash, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(input.id)
        .bind(input.user_id)
        .bind(input.name)
        .bind(input.token_hash)
        .bind(input.now)
        .bind(input.now)
        .execute(&self.pool)
        .await?;
        self.device_by_id(input.id).await
    }

    async fn find_device_by_token_hash(
        &self,
        token_hash: &[u8],
        now: NaiveDateTime,
    ) -> sqlx::Result<Option<AuthenticatedDevice>> {
        sqlx::query("UPDATE devices SET last_used_at = ? WHERE token_hash = ?")
            .bind(now)
            .bind(token_hash)
            .execute(&self.pool)
            .await?;
        sqlx::query_as("SELECT id, user_id, name FROM devices WHERE token_hash = ?")
            .bind(token_hash)
            .fetch_optional(&self.pool)
            .await
    }

    async fn list_devices(&self, user_id: &str) -> sqlx::Result<Vec<Device>> {
        sqlx::query_as(
            "SELECT id, name, created_at, updated_at, last_used_at FROM devices WHERE user_id = ? ORDER BY created_at, id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
    }

    async fn rename_device(&self, user_id: &str, id: &str, name: &str) -> sqlx::Result<Option<Device>> {
        sqlx::query("UPDATE devices SET name = ? WHERE id = ? AND user_id = ?")
            .bind(name)
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        sqlx::query_as(
            "SELECT id, name, created_at, updated_at, last_used_at FROM devices WHERE id = ? AND user_id = ?",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
    }

    async fn delete_device(&self, user_id: &str, id: &str) -> sqlx::Result<bool> {
        Ok(sqlx::query("DELETE FROM devices WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await?
            .rows_affected()
            > 0)
    }

    async fn create_url(&self, input: CreateUrl<'_>) -> sqlx::Result<SharedUrl> {
        sqlx::query(
            "INSERT INTO shared_urls (id, user_id, source_device_id, source_device_name, url, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(input.id)
        .bind(input.user_id)
        .bind(input.source_device_id)
        .bind(input.source_device_name)
        .bind(input.url)
        .bind(input.now)
        .bind(input.expires_at)
        .execute(&self.pool)
        .await?;
        sqlx::query_as(
            "SELECT id, url, source_device_id, source_device_name, created_at, expires_at FROM shared_urls WHERE id = ?",
        )
        .bind(input.id)
        .fetch_one(&self.pool)
        .await
    }

    async fn get_latest_url(&self, user_id: &str, now: NaiveDateTime) -> sqlx::Result<Option<SharedUrl>> {
        sqlx::query_as(
            "SELECT id, url, source_device_id, source_device_name, created_at, expires_at FROM shared_urls WHERE user_id = ? AND expires_at > ? ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(user_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
    }

    async fn list_urls(
        &self,
        user_id: &str,
        now: NaiveDateTime,
        limit: u32,
        cursor: Option<&UrlCursor>,
    ) -> sqlx::Result<Vec<SharedUrl>> {
        let take = limit + 1;
        if let Some(cursor) = cursor {
            return sqlx::query_as(
                "SELECT id, url, source_device_id, source_device_name, created_at, expires_at FROM shared_urls WHERE user_id = ? AND expires_at > ? AND (created_at < ? OR (created_at = ? AND id < ?)) ORDER BY created_at DESC, id DESC LIMIT ?",
            )
            .bind(user_id)
            .bind(now)
            .bind(cursor.created_at.naive_utc())
            .bind(cursor.created_at.naive_utc())
            .bind(&cursor.id)
            .bind(take)
            .fetch_all(&self.pool)
            .await;
        }
        sqlx::query_as(
            "SELECT id, url, source_device_id, source_device_name, created_at, expires_at FROM shared_urls WHERE user_id = ? AND expires_at > ? ORDER BY created_at DESC, id DESC LIMIT ?",
        )
        .bind(user_id)
        .bind(now)
        .bind(take)
        .fetch_all(&self.pool)
        .await
    }

    async fn delete_url(&self, user_id: &str, id: &str) -> sqlx::Result<bool> {
        Ok(sqlx::query("DELETE FROM shared_urls WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await?
            .rows_affected()
            > 0)
    }

    async fn delete_expired_urls(&self, now: NaiveDateTime) -> sqlx::Result<u64> {
        Ok(sqlx::query("DELETE FROM shared_urls WHERE expires_at <= ?")
            .bind(now)
            .execute(&self.pool)
            .await?
            .rows_affected())
    }

    async fn create_memo_bundle(&self, input: CreateMemoBundle) -> sqlx::Result<Vec<CreatedMemoItem>> {
        let mut transaction = self.pool.begin().await?;
        let mut created = Vec::with_capacity(input.urls.len() + usize::from(input.memo.is_some()));

        for url in input.urls {
            sqlx::query(
                "INSERT INTO shared_urls (id, user_id, source_device_id, source_device_name, url, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&url.id)
            .bind(&url.user_id)
            .bind(&url.source_device_id)
            .bind(&url.source_device_name)
            .bind(&url.url)
            .bind(url.now)
            .bind(url.expires_at)
            .execute(&mut *transaction)
            .await?;
            let url = sqlx::query_as(
                "SELECT id, url, source_device_id, source_device_name, created_at, expires_at FROM shared_urls WHERE id = ?",
            )
            .bind(&url.id)
            .fetch_one(&mut *transaction)
            .await?;
            created.push(CreatedMemoItem::Url(url));
        }

        if let Some(memo) = input.memo {
            sqlx::query(
                "INSERT INTO shared_memos (id, user_id, source_device_id, source_device_name, content, created_at, updated_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&memo.id)
            .bind(&memo.user_id)
            .bind(&memo.source_device_id)
            .bind(&memo.source_device_name)
            .bind(&memo.content)
            .bind(memo.now)
            .bind(memo.now)
            .bind(memo.expires_at)
            .execute(&mut *transaction)
            .await?;
            let memo = sqlx::query_as(
                "SELECT id, content, source_device_id, source_device_name, created_at, updated_at, expires_at FROM shared_memos WHERE id = ?",
            )
            .bind(&memo.id)
            .fetch_one(&mut *transaction)
            .await?;
            created.push(CreatedMemoItem::Memo(memo));
        }

        transaction.commit().await?;
        Ok(created)
    }

    async fn get_latest_memo(&self, user_id: &str, now: NaiveDateTime) -> sqlx::Result<Option<SharedMemo>> {
        sqlx::query_as(
            "SELECT id, content, source_device_id, source_device_name, created_at, updated_at, expires_at FROM shared_memos WHERE user_id = ? AND expires_at > ? ORDER BY updated_at DESC, id DESC LIMIT 1",
        )
        .bind(user_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
    }

    async fn list_memos(
        &self,
        user_id: &str,
        now: NaiveDateTime,
        limit: u32,
        cursor: Option<&UrlCursor>,
    ) -> sqlx::Result<Vec<SharedMemo>> {
        let take = limit + 1;
        if let Some(cursor) = cursor {
            return sqlx::query_as(
                "SELECT id, content, source_device_id, source_device_name, created_at, updated_at, expires_at FROM shared_memos WHERE user_id = ? AND expires_at > ? AND (created_at < ? OR (created_at = ? AND id < ?)) ORDER BY created_at DESC, id DESC LIMIT ?",
            )
            .bind(user_id)
            .bind(now)
            .bind(cursor.created_at.naive_utc())
            .bind(cursor.created_at.naive_utc())
            .bind(&cursor.id)
            .bind(take)
            .fetch_all(&self.pool)
            .await;
        }
        sqlx::query_as(
            "SELECT id, content, source_device_id, source_device_name, created_at, updated_at, expires_at FROM shared_memos WHERE user_id = ? AND expires_at > ? ORDER BY created_at DESC, id DESC LIMIT ?",
        )
        .bind(user_id)
        .bind(now)
        .bind(take)
        .fetch_all(&self.pool)
        .await
    }

    async fn update_memo(
        &self,
        user_id: &str,
        id: &str,
        content: &str,
        now: NaiveDateTime,
        expires_at: NaiveDateTime,
    ) -> sqlx::Result<Option<SharedMemo>> {
        sqlx::query("UPDATE shared_memos SET content = ?, updated_at = ?, expires_at = ? WHERE id = ? AND user_id = ?")
            .bind(content)
            .bind(now)
            .bind(expires_at)
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        sqlx::query_as(
            "SELECT id, content, source_device_id, source_device_name, created_at, updated_at, expires_at FROM shared_memos WHERE id = ? AND user_id = ?",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
    }

    async fn delete_memo(&self, user_id: &str, id: &str) -> sqlx::Result<bool> {
        Ok(sqlx::query("DELETE FROM shared_memos WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await?
            .rows_affected()
            > 0)
    }

    async fn delete_expired_memos(&self, now: NaiveDateTime) -> sqlx::Result<u64> {
        Ok(sqlx::query("DELETE FROM shared_memos WHERE expires_at <= ?")
            .bind(now)
            .execute(&self.pool)
            .await?
            .rows_affected())
    }

    async fn create_file_and_evict(&self, input: CreateFile, maximum_bytes: u64) -> sqlx::Result<CreatedFile> {
        let _guard = self.file_write_lock.lock().await;
        let mut delay = std::time::Duration::from_millis(25);
        for attempt in 0..5 {
            match self.create_file_and_evict_once(input.clone(), maximum_bytes).await {
                Ok(created) => return Ok(created),
                Err(error) if is_deadlock(&error) && attempt < 4 => {
                    tracing::warn!(attempt, "retrying file upload after a database deadlock");
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("the retry loop always returns")
    }

    async fn create_file_and_evict_once(&self, input: CreateFile, maximum_bytes: u64) -> sqlx::Result<CreatedFile> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("INSERT IGNORE INTO shared_file_usage (user_id, bytes) VALUES (?, 0)")
            .bind(&input.user_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("SELECT bytes FROM shared_file_usage WHERE user_id = ? FOR UPDATE")
            .bind(&input.user_id)
            .fetch_one(&mut *transaction)
            .await?;
        sqlx::query("INSERT INTO shared_files (id, user_id, source_device_id, source_device_name, name, content_type, size, storage_key, created_at, updated_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&input.id).bind(&input.user_id).bind(&input.source_device_id).bind(&input.source_device_name)
            .bind(&input.name).bind(&input.content_type).bind(input.size).bind(&input.storage_key)
            .bind(input.now).bind(input.now).bind(input.expires_at)
            .execute(&mut *transaction).await?;
        let records: Vec<FileRecord> = sqlx::query_as::<_, FileRow>("SELECT id, name, content_type, size, source_device_id, source_device_name, created_at, updated_at, expires_at, storage_key FROM shared_files WHERE user_id = ? AND expires_at > ? ORDER BY updated_at, (id = ?) ASC, id")
            .bind(&input.user_id).bind(input.now).bind(&input.id).fetch_all(&mut *transaction).await?
            .into_iter().map(FileRecord::from).collect();
        let mut total: u64 = records.iter().map(|record| record.file.size).sum();
        let mut evicted = Vec::new();
        for record in records {
            if total <= maximum_bytes {
                break;
            }
            total -= record.file.size;
            sqlx::query("DELETE FROM shared_files WHERE id = ?")
                .bind(&record.file.id)
                .execute(&mut *transaction)
                .await?;
            evicted.push(record);
        }
        sqlx::query("UPDATE shared_file_usage SET bytes = ? WHERE user_id = ?")
            .bind(total)
            .bind(&input.user_id)
            .execute(&mut *transaction)
            .await?;
        let file: SharedFile = sqlx::query_as("SELECT id, name, content_type, size, source_device_id, source_device_name, created_at, updated_at, expires_at FROM shared_files WHERE id = ?")
            .bind(&input.id).fetch_one(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(CreatedFile { file, evicted })
    }

    async fn get_file(&self, user_id: &str, id: &str, now: NaiveDateTime) -> sqlx::Result<Option<FileRecord>> {
        let row = sqlx::query_as::<_, FileRow>("SELECT id, name, content_type, size, source_device_id, source_device_name, created_at, updated_at, expires_at, storage_key FROM shared_files WHERE user_id = ? AND id = ? AND expires_at > ?")
            .bind(user_id).bind(id).bind(now).fetch_optional(&self.pool).await?;
        Ok(row.map(FileRecord::from))
    }

    async fn get_latest_file(&self, user_id: &str, now: NaiveDateTime) -> sqlx::Result<Option<SharedFile>> {
        sqlx::query_as("SELECT id, name, content_type, size, source_device_id, source_device_name, created_at, updated_at, expires_at FROM shared_files WHERE user_id = ? AND expires_at > ? ORDER BY updated_at DESC, id DESC LIMIT 1")
            .bind(user_id).bind(now).fetch_optional(&self.pool).await
    }

    async fn list_files(
        &self,
        user_id: &str,
        now: NaiveDateTime,
        limit: u32,
        cursor: Option<&UrlCursor>,
    ) -> sqlx::Result<Vec<SharedFile>> {
        let take = limit + 1;
        if let Some(cursor) = cursor {
            return sqlx::query_as("SELECT id, name, content_type, size, source_device_id, source_device_name, created_at, updated_at, expires_at FROM shared_files WHERE user_id = ? AND expires_at > ? AND (created_at < ? OR (created_at = ? AND id < ?)) ORDER BY created_at DESC, id DESC LIMIT ?")
                .bind(user_id).bind(now).bind(cursor.created_at.naive_utc()).bind(cursor.created_at.naive_utc()).bind(&cursor.id).bind(take).fetch_all(&self.pool).await;
        }
        sqlx::query_as("SELECT id, name, content_type, size, source_device_id, source_device_name, created_at, updated_at, expires_at FROM shared_files WHERE user_id = ? AND expires_at > ? ORDER BY created_at DESC, id DESC LIMIT ?")
            .bind(user_id).bind(now).bind(take).fetch_all(&self.pool).await
    }

    async fn rename_file(
        &self,
        user_id: &str,
        id: &str,
        name: &str,
        now: NaiveDateTime,
        expires_at: NaiveDateTime,
    ) -> sqlx::Result<Option<SharedFile>> {
        let _guard = self.file_write_lock.lock().await;
        sqlx::query("UPDATE shared_files SET name = ?, updated_at = ?, expires_at = ? WHERE id = ? AND user_id = ?")
            .bind(name)
            .bind(now)
            .bind(expires_at)
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        sqlx::query_as("SELECT id, name, content_type, size, source_device_id, source_device_name, created_at, updated_at, expires_at FROM shared_files WHERE id = ? AND user_id = ?")
            .bind(id).bind(user_id).fetch_optional(&self.pool).await
    }

    async fn delete_file(&self, user_id: &str, id: &str) -> sqlx::Result<Option<FileRecord>> {
        let _guard = self.file_write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SELECT bytes FROM shared_file_usage WHERE user_id = ? FOR UPDATE")
            .bind(user_id)
            .fetch_optional(&mut *transaction)
            .await?;
        let row = sqlx::query_as::<_, FileRow>("SELECT id, name, content_type, size, source_device_id, source_device_name, created_at, updated_at, expires_at, storage_key FROM shared_files WHERE id = ? AND user_id = ? FOR UPDATE")
            .bind(id).bind(user_id).fetch_optional(&mut *transaction).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let record = FileRecord::from(row);
        sqlx::query("DELETE FROM shared_files WHERE id = ?")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("UPDATE shared_file_usage SET bytes = GREATEST(bytes - ?, 0) WHERE user_id = ?")
            .bind(record.file.size)
            .bind(user_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(Some(record))
    }

    async fn delete_expired_files(&self, now: NaiveDateTime) -> sqlx::Result<Vec<FileRecord>> {
        let _guard = self.file_write_lock.lock().await;
        let records: Vec<FileRecord> = sqlx::query_as::<_, FileRow>("SELECT id, name, content_type, size, source_device_id, source_device_name, created_at, updated_at, expires_at, storage_key FROM shared_files WHERE expires_at <= ?")
            .bind(now).fetch_all(&self.pool).await?.into_iter().map(FileRecord::from).collect();
        sqlx::query("DELETE FROM shared_files WHERE expires_at <= ?")
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(records)
    }

    async fn clear_files(&self) -> sqlx::Result<()> {
        let _guard = self.file_write_lock.lock().await;
        sqlx::query("DELETE FROM shared_files").execute(&self.pool).await?;
        sqlx::query("DELETE FROM shared_file_usage").execute(&self.pool).await?;
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct FileRow {
    id: String,
    name: String,
    content_type: String,
    size: u64,
    source_device_id: Option<String>,
    source_device_name: String,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    expires_at: NaiveDateTime,
    storage_key: String,
}

impl From<FileRow> for FileRecord {
    fn from(row: FileRow) -> Self {
        Self {
            storage_key: row.storage_key,
            file: SharedFile {
                id: row.id,
                name: row.name,
                content_type: row.content_type,
                size: row.size,
                source_device_id: row.source_device_id,
                source_device_name: row.source_device_name,
                created_at: row.created_at,
                updated_at: row.updated_at,
                expires_at: row.expires_at,
            },
        }
    }
}

fn is_deadlock(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database) if database.code().as_deref() == Some("1213"))
}

impl MySqlRepository {
    async fn device_by_id(&self, id: &str) -> sqlx::Result<Device> {
        sqlx::query_as("SELECT id, name, created_at, updated_at, last_used_at FROM devices WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
    }
}
