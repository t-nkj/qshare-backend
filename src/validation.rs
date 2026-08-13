use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, SecondsFormat, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::{error::ApiError, model::UrlCursor};

const DEVICE_NAME_MAX_LENGTH: usize = 64;
const URL_MAX_LENGTH: usize = 4096;
const MEMO_CONTENT_MAX_LENGTH: usize = 10_000;

pub fn device_name(body: &Value) -> Result<String, ApiError> {
    let Some(value) = body.get("name").and_then(Value::as_str) else {
        return Err(ApiError::bad_request("INVALID_DEVICE_NAME", "name must be a string"));
    };
    let name = value.trim();
    if name.is_empty() || name.chars().count() > DEVICE_NAME_MAX_LENGTH {
        return Err(ApiError::bad_request(
            "INVALID_DEVICE_NAME",
            "name must contain between 1 and 64 characters",
        ));
    }
    Ok(name.to_owned())
}

pub fn http_url(body: &Value) -> Result<String, ApiError> {
    let Some(value) = body.get("url").and_then(Value::as_str) else {
        return Err(ApiError::bad_request(
            "INVALID_URL",
            "url must be a non-empty string of at most 4096 characters",
        ));
    };
    if value.is_empty() || value.len() > URL_MAX_LENGTH {
        return Err(ApiError::bad_request(
            "INVALID_URL",
            "url must be a non-empty string of at most 4096 characters",
        ));
    }
    if value.trim() != value {
        return Err(ApiError::bad_request(
            "INVALID_URL",
            "url must not have leading or trailing whitespace",
        ));
    }
    validate_http_url(value)?;
    Ok(value.to_owned())
}

pub fn validate_http_url(value: &str) -> Result<(), ApiError> {
    if value.is_empty() || value.len() > URL_MAX_LENGTH {
        return Err(ApiError::bad_request(
            "INVALID_URL",
            "url must be a non-empty string of at most 4096 characters",
        ));
    }
    if value.trim() != value {
        return Err(ApiError::bad_request(
            "INVALID_URL",
            "url must not have leading or trailing whitespace",
        ));
    }
    let parsed =
        Url::parse(value).map_err(|_| ApiError::bad_request("INVALID_URL", "url must be an absolute HTTP(S) URL"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
        return Err(ApiError::bad_request(
            "INVALID_URL",
            "url must be an absolute HTTP(S) URL",
        ));
    }
    Ok(())
}

pub fn memo_content(body: &Value) -> Result<String, ApiError> {
    let Some(value) = body.get("content").and_then(Value::as_str) else {
        return Err(ApiError::bad_request(
            "INVALID_MEMO_CONTENT",
            "content must be a string",
        ));
    };
    if value.trim().is_empty() || value.chars().count() > MEMO_CONTENT_MAX_LENGTH {
        return Err(ApiError::bad_request(
            "INVALID_MEMO_CONTENT",
            "content must contain between 1 and 10000 characters",
        ));
    }
    Ok(value.to_owned())
}

pub fn auto_detect_urls(body: &Value) -> Result<bool, ApiError> {
    match body.get("autoDetectUrls") {
        None => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(ApiError::bad_request(
            "INVALID_AUTO_DETECT_URLS",
            "autoDetectUrls must be a boolean",
        )),
    }
}

pub fn extract_http_urls(content: &str) -> Vec<String> {
    let matcher = Regex::new(r#"(?i)https?://[^\s<>()\[\]{}\"']+"#).expect("static URL expression must be valid");
    let mut urls = Vec::new();
    for item in matcher.find_iter(content) {
        let url = item
            .as_str()
            .trim_end_matches(['.', ',', '!', '?', ':', ';'])
            .to_owned();
        if validate_http_url(&url).is_ok() && !urls.contains(&url) {
            urls.push(url);
        }
    }
    urls
}

pub fn is_bare_http_url(content: &str, urls: &[String]) -> bool {
    let trimmed = content.trim();
    urls.len() == 1 && urls[0] == trimmed
}

pub fn limit(value: Option<&str>) -> Result<u32, ApiError> {
    let Some(value) = value else {
        return Ok(50);
    };
    if value.is_empty() || !value.bytes().all(|character| character.is_ascii_digit()) {
        return Err(invalid_limit());
    }
    let value = value.parse().map_err(|_| invalid_limit())?;
    if !(1..=100).contains(&value) {
        return Err(invalid_limit());
    }
    Ok(value)
}

fn invalid_limit() -> ApiError {
    ApiError::bad_request("INVALID_LIMIT", "limit must be an integer between 1 and 100")
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorPayload {
    id: String,
    created_at: String,
}

pub fn encode_cursor(id: &str, created_at: chrono::NaiveDateTime) -> String {
    let payload = CursorPayload {
        id: id.to_owned(),
        created_at: created_at.and_utc().to_rfc3339_opts(SecondsFormat::Millis, true),
    };
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).expect("cursor serialization cannot fail"))
}

pub fn decode_cursor(value: Option<&str>) -> Result<Option<UrlCursor>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let payload = URL_SAFE_NO_PAD
        .decode(value)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<CursorPayload>(&bytes).ok())
        .and_then(|payload| {
            DateTime::parse_from_rfc3339(&payload.created_at)
                .ok()
                .map(|created_at| UrlCursor {
                    id: payload.id,
                    created_at: created_at.with_timezone(&Utc),
                })
        })
        .ok_or_else(|| ApiError::bad_request("INVALID_CURSOR", "cursor is invalid"))?;
    Ok(Some(payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_is_compatible_with_original_json_shape() {
        let date = DateTime::parse_from_rfc3339("2026-08-12T00:00:00.000Z")
            .unwrap()
            .naive_utc();
        let encoded = encode_cursor("id", date);
        let decoded = decode_cursor(Some(&encoded)).unwrap().unwrap();
        assert_eq!(decoded.id, "id");
        assert_eq!(decoded.created_at.naive_utc(), date);
    }
}
