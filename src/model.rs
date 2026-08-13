use chrono::NaiveDateTime;
use serde::Serialize;

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    #[serde(serialize_with = "serialize_datetime")]
    pub created_at: NaiveDateTime,
    #[serde(serialize_with = "serialize_datetime")]
    pub updated_at: NaiveDateTime,
    #[serde(serialize_with = "serialize_optional_datetime")]
    pub last_used_at: Option<NaiveDateTime>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct AuthenticatedDevice {
    pub id: String,
    pub user_id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SharedUrl {
    pub id: String,
    pub url: String,
    pub source_device_id: Option<String>,
    pub source_device_name: String,
    #[serde(serialize_with = "serialize_datetime")]
    pub created_at: NaiveDateTime,
    #[serde(serialize_with = "serialize_datetime")]
    pub expires_at: NaiveDateTime,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SharedMemo {
    pub id: String,
    pub content: String,
    pub source_device_id: Option<String>,
    pub source_device_name: String,
    #[serde(serialize_with = "serialize_datetime")]
    pub created_at: NaiveDateTime,
    #[serde(serialize_with = "serialize_datetime")]
    pub updated_at: NaiveDateTime,
    #[serde(serialize_with = "serialize_datetime")]
    pub expires_at: NaiveDateTime,
}

#[derive(Clone, Debug, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UrlCursor {
    pub id: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub created_at: chrono::DateTime<chrono::Utc>,
}

fn serialize_datetime<S>(value: &NaiveDateTime, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    value
        .and_utc()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        .serialize(serializer)
}

fn serialize_optional_datetime<S>(value: &Option<NaiveDateTime>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(value) => serialize_datetime(value, serializer),
        None => serializer.serialize_none(),
    }
}
