use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::{MySqlPool, mysql::MySqlPoolOptions};

use crate::model::{AuthenticatedDevice, Device, SharedMemo, SharedUrl, UrlCursor};

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
}

pub struct MySqlRepository {
    pool: MySqlPool,
}

impl MySqlRepository {
    pub async fn connect(database_url: &str) -> sqlx::Result<Self> {
        let pool = MySqlPoolOptions::new()
            .min_connections(0)
            .max_connections(5)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(database_url)
            .await?;
        Ok(Self { pool })
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
            "SELECT id, content, source_device_id, source_device_name, created_at, updated_at, expires_at FROM shared_memos WHERE user_id = ? AND expires_at > ? ORDER BY created_at DESC, id DESC LIMIT 1",
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
}

impl MySqlRepository {
    async fn device_by_id(&self, id: &str) -> sqlx::Result<Device> {
        sqlx::query_as("SELECT id, name, created_at, updated_at, last_used_at FROM devices WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
    }
}
