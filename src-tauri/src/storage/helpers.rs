use chrono::{DateTime, Utc};
use rusqlite::types::Type;

use crate::types::{PlatformAccount, TaskRecord};

pub(super) fn dt_to_str(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

/// 把 RFC3339 字符串解析成 `DateTime<Utc>`。
/// 解析失败时返回 `rusqlite::Error::FromSqlConversionFailure`，由 `?` 冒泡到 `AppError::Database`，
/// 避免单行坏数据让持锁线程 panic 进而 poison 全局 storage Mutex。
pub(super) fn str_to_dt(s: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e)))
}

pub(super) fn str_to_opt_dt(s: Option<String>) -> rusqlite::Result<Option<DateTime<Utc>>> {
    s.map(|v| str_to_dt(&v)).transpose()
}

pub(super) fn enum_to_str<T: serde::Serialize>(v: &T) -> String {
    let json = serde_json::to_value(v).expect("enum serialize");
    json.as_str()
        .expect("enum must serialize to string")
        .to_owned()
}

/// 把数据库里的字符串反序列化为枚举。
/// 反序列化失败时返回 `rusqlite::Error::FromSqlConversionFailure`，由 `?` 冒泡到 `AppError::Database`，
/// 避免 `login_state` / `task_kind` / `status` 等字段出现未知值时让持锁线程 panic。
pub(super) fn str_to_enum<T: serde::de::DeserializeOwned>(s: &str) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s.to_owned()))
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e)))
}

pub(super) fn row_to_account(row: &rusqlite::Row) -> rusqlite::Result<PlatformAccount> {
    Ok(PlatformAccount {
        id: row.get("id")?,
        platform: row.get("platform")?,
        name: row.get("name")?,
        notes: row.get("notes")?,
        login_state: str_to_enum(&row.get::<_, String>("login_state")?)?,
        login_state_reason: row.get("login_state_reason")?,
        login_state_updated_at: str_to_dt(&row.get::<_, String>("login_state_updated_at")?)?,
        cookies: row.get("cookies")?,
        api_token: row.get("api_token")?,
        refresh_token: row.get("refresh_token")?,
        token_expires_at: str_to_opt_dt(row.get("token_expires_at")?)?,
        profile_dir: row.get("profile_dir")?,
        tier: row.get("tier")?,
        concurrency_limit: row.get("concurrency_limit")?,
        daily_limit: row.get("daily_limit")?,
        total_limit: row.get("total_limit")?,
        task_interval_seconds: row.get("task_interval_seconds")?,
        used_today: row.get("used_today")?,
        used_total: row.get("used_total")?,
        usage_date: row.get("usage_date")?,
        cooldown_until: str_to_opt_dt(row.get("cooldown_until")?)?,
        user_id: row.get("user_id")?,
        username: row.get("username")?,
        email: row.get("email")?,
        avatar_url: row.get("avatar_url")?,
        password_encrypted: row.get("password_encrypted")?,
        created_at: str_to_dt(&row.get::<_, String>("created_at")?)?,
        last_check_at: str_to_opt_dt(row.get("last_check_at")?)?,
        last_refreshed_at: str_to_opt_dt(row.get("last_refreshed_at")?)?,
        last_used_at: str_to_opt_dt(row.get("last_used_at")?)?,
    })
}

pub(super) fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<TaskRecord> {
    Ok(TaskRecord {
        id: row.get("id")?,
        platform: row.get("platform")?,
        task_kind: str_to_enum(&row.get::<_, String>("task_kind")?)?,
        model_id: row.get("model_id")?,
        prompt: row.get("prompt")?,
        account_id: row.get("account_id")?,
        status: str_to_enum(&row.get::<_, String>("status")?)?,
        result_url: row.get("result_url")?,
        storage_key: row.get("storage_key")?,
        error_message: row.get("error_message")?,
        result_data: row.get("result_data")?,
        created_at: str_to_dt(&row.get::<_, String>("created_at")?)?,
        completed_at: str_to_opt_dt(row.get("completed_at")?)?,
    })
}
