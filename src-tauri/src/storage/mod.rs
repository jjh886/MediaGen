pub(crate) mod helpers;
mod logs;
mod tasks;
use helpers::{dt_to_str, enum_to_str, row_to_account};
#[cfg(test)]
mod tests;

use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::AppError;
use crate::types::*;

const CORE_SCHEMA_SQL: &str = "CREATE TABLE IF NOT EXISTS accounts (
    id                    TEXT PRIMARY KEY,
    platform              TEXT NOT NULL,
    name                  TEXT NOT NULL,
    notes                 TEXT NOT NULL DEFAULT '',
    login_state           TEXT NOT NULL DEFAULT 'available',
    login_state_reason    TEXT NOT NULL DEFAULT '',
    login_state_updated_at TEXT NOT NULL,
    cookies               TEXT NOT NULL DEFAULT '',
    api_token             TEXT NOT NULL DEFAULT '',
    refresh_token         TEXT NOT NULL DEFAULT '',
    token_expires_at      TEXT,
    profile_dir           TEXT NOT NULL DEFAULT '',
    tier                  TEXT NOT NULL DEFAULT '',
    concurrency_limit     INTEGER NOT NULL DEFAULT 1,
    daily_limit           INTEGER NOT NULL DEFAULT 0,
    total_limit           INTEGER NOT NULL DEFAULT 0,
    task_interval_seconds INTEGER NOT NULL DEFAULT 0,
    used_today            INTEGER NOT NULL DEFAULT 0,
    used_total            INTEGER NOT NULL DEFAULT 0,
    usage_date            TEXT NOT NULL DEFAULT '',
    cooldown_until        TEXT,
    user_id               TEXT NOT NULL DEFAULT '',
    username              TEXT NOT NULL DEFAULT '',
    email                 TEXT NOT NULL DEFAULT '',
    avatar_url            TEXT NOT NULL DEFAULT '',
    password_encrypted    TEXT NOT NULL DEFAULT '',
    created_at            TEXT NOT NULL,
    last_check_at         TEXT,
    last_refreshed_at     TEXT,
    last_used_at          TEXT
);
CREATE TABLE IF NOT EXISTS tasks (
    id              TEXT PRIMARY KEY,
    platform        TEXT NOT NULL,
    task_kind       TEXT NOT NULL,
    model_id        TEXT NOT NULL,
    prompt          TEXT NOT NULL DEFAULT '',
    account_id      TEXT,
    status          TEXT NOT NULL DEFAULT 'pending',
    result_url      TEXT,
    storage_key     TEXT,
    error_message   TEXT,
    created_at      TEXT NOT NULL,
    completed_at    TEXT
);
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS logs (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp  TEXT NOT NULL,
    module     TEXT NOT NULL,
    level      TEXT NOT NULL,
    summary    TEXT NOT NULL,
    detail     TEXT,
    task_id    TEXT
);
CREATE INDEX IF NOT EXISTS idx_logs_timestamp ON logs(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_logs_module ON logs(module);
CREATE INDEX IF NOT EXISTS idx_logs_level ON logs(level);";

pub struct Storage {
    conn: Connection,
}

impl Storage {
    pub fn new(db_path: impl AsRef<Path>) -> Result<Self, AppError> {
        let conn = Connection::open(db_path)?;
        initialize_connection(&conn)?;
        Ok(Self { conn })
    }

    // ── 账号 ──

    pub fn insert_account(&self, a: &PlatformAccount) -> Result<(), AppError> {
        self.conn.execute(
            "INSERT INTO accounts (
                id, platform, name, notes, login_state, login_state_reason,
                login_state_updated_at, cookies, api_token, refresh_token,
                token_expires_at, profile_dir, tier, concurrency_limit,
                daily_limit, total_limit, task_interval_seconds, used_today,
                used_total, usage_date, cooldown_until, user_id, username, email,
                avatar_url, password_encrypted, created_at, last_check_at,
                last_refreshed_at, last_used_at
            ) VALUES (
                ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,
                ?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30
            )",
            params![
                a.id,
                a.platform,
                a.name,
                a.notes,
                enum_to_str(&a.login_state),
                a.login_state_reason,
                dt_to_str(&a.login_state_updated_at),
                a.cookies,
                a.api_token,
                a.refresh_token,
                a.token_expires_at.map(|t| dt_to_str(&t)),
                a.profile_dir,
                a.tier,
                a.concurrency_limit,
                a.daily_limit,
                a.total_limit,
                a.task_interval_seconds,
                a.used_today,
                a.used_total,
                a.usage_date,
                a.cooldown_until.map(|t| dt_to_str(&t)),
                a.user_id,
                a.username,
                a.email,
                a.avatar_url,
                a.password_encrypted,
                dt_to_str(&a.created_at),
                a.last_check_at.map(|t| dt_to_str(&t)),
                a.last_refreshed_at.map(|t| dt_to_str(&t)),
                a.last_used_at.map(|t| dt_to_str(&t)),
            ],
        )?;
        Ok(())
    }

    pub fn get_account(&self, id: &str) -> Result<Option<PlatformAccount>, AppError> {
        self.conn
            .query_row("SELECT * FROM accounts WHERE id = ?1", params![id], |row| {
                row_to_account(row)
            })
            .optional()
            .map_err(AppError::from)
    }

    pub fn list_accounts_by_platform(
        &self,
        platform: &str,
    ) -> Result<Vec<PlatformAccount>, AppError> {
        self.refresh_stale_daily_usage(&current_usage_date())?;
        let mut stmt = self.conn.prepare(
            "SELECT * FROM accounts WHERE platform = ?1 ORDER BY created_at DESC, id ASC",
        )?;
        let rows = stmt.query_map(params![platform], |row| row_to_account(row))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn update_account(&self, a: &PlatformAccount) -> Result<(), AppError> {
        self.conn.execute(
            "UPDATE accounts SET
                platform=?2, name=?3, notes=?4, login_state=?5,
                login_state_reason=?6, login_state_updated_at=?7,
                cookies=?8, api_token=?9, refresh_token=?10,
                token_expires_at=?11, profile_dir=?12, tier=?13,
                concurrency_limit=?14, daily_limit=?15, total_limit=?16,
                task_interval_seconds=?17, used_today=?18, used_total=?19,
                usage_date=?20, cooldown_until=?21, user_id=?22, username=?23,
                email=?24, avatar_url=?25, password_encrypted=?26, created_at=?27,
                last_check_at=?28, last_refreshed_at=?29, last_used_at=?30
            WHERE id = ?1",
            params![
                a.id,
                a.platform,
                a.name,
                a.notes,
                enum_to_str(&a.login_state),
                a.login_state_reason,
                dt_to_str(&a.login_state_updated_at),
                a.cookies,
                a.api_token,
                a.refresh_token,
                a.token_expires_at.map(|t| dt_to_str(&t)),
                a.profile_dir,
                a.tier,
                a.concurrency_limit,
                a.daily_limit,
                a.total_limit,
                a.task_interval_seconds,
                a.used_today,
                a.used_total,
                a.usage_date,
                a.cooldown_until.map(|t| dt_to_str(&t)),
                a.user_id,
                a.username,
                a.email,
                a.avatar_url,
                a.password_encrypted,
                dt_to_str(&a.created_at),
                a.last_check_at.map(|t| dt_to_str(&t)),
                a.last_refreshed_at.map(|t| dt_to_str(&t)),
                a.last_used_at.map(|t| dt_to_str(&t)),
            ],
        )?;
        Ok(())
    }

    pub fn update_login_state(
        &self,
        id: &str,
        state: LoginState,
        reason: &str,
    ) -> Result<(), AppError> {
        self.conn.execute(
            "UPDATE accounts SET login_state=?1, login_state_reason=?2,
             login_state_updated_at=?3 WHERE id=?4",
            params![enum_to_str(&state), reason, dt_to_str(&Utc::now()), id],
        )?;
        Ok(())
    }

    /// 同时更新 login_state 和 cooldown_until
    pub fn update_login_state_with_cooldown(
        &self,
        id: &str,
        state: LoginState,
        reason: &str,
        cooldown_until: Option<chrono::DateTime<Utc>>,
    ) -> Result<(), AppError> {
        self.conn.execute(
            "UPDATE accounts SET login_state=?1, login_state_reason=?2,
             login_state_updated_at=?3, cooldown_until=?4 WHERE id=?5",
            params![
                enum_to_str(&state),
                reason,
                dt_to_str(&Utc::now()),
                cooldown_until.map(|t| dt_to_str(&t)),
                id,
            ],
        )?;
        Ok(())
    }

    /// 将所有 cooldown 已过期的账号恢复为 Available
    pub fn recover_expired_cooldowns(&self) -> Result<u64, AppError> {
        let now = dt_to_str(&Utc::now());
        let changed = self.conn.execute(
            "UPDATE accounts SET login_state='available', login_state_reason='冷却已过期，自动恢复',
             login_state_updated_at=?1, cooldown_until=NULL
             WHERE login_state='cooldown' AND cooldown_until IS NOT NULL AND cooldown_until < ?1",
            params![now],
        )?;
        Ok(changed as u64)
    }

    pub fn update_usage(&self, id: &str, used_today: i32, used_total: i32) -> Result<(), AppError> {
        self.conn.execute(
            "UPDATE accounts SET used_today=?1, used_total=?2 WHERE id=?3",
            params![used_today, used_total, id],
        )?;
        Ok(())
    }

    pub fn reset_daily_usage(&self, date: &str) -> Result<u64, AppError> {
        let changed = self.conn.execute(
            "UPDATE accounts SET used_today=0, usage_date=?1 WHERE usage_date != ?1",
            params![date],
        )?;
        Ok(changed as u64)
    }

    pub fn refresh_stale_daily_usage(&self, date: &str) -> Result<u64, AppError> {
        let changed = self.conn.execute(
            "UPDATE accounts SET
                used_today=0,
                usage_date=?1,
                login_state=CASE
                    WHEN login_state='daily_quota_exhausted' THEN 'available'
                    ELSE login_state
                END,
                login_state_reason=CASE
                    WHEN login_state='daily_quota_exhausted' THEN '每日配额已重置'
                    ELSE login_state_reason
                END,
                login_state_updated_at=CASE
                    WHEN login_state='daily_quota_exhausted' THEN ?2
                    ELSE login_state_updated_at
                END
             WHERE usage_date != ?1",
            params![date, dt_to_str(&Utc::now())],
        )?;
        Ok(changed as u64)
    }

    pub fn list_all_accounts(&self) -> Result<Vec<PlatformAccount>, AppError> {
        self.refresh_stale_daily_usage(&current_usage_date())?;
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM accounts ORDER BY created_at DESC, id ASC")?;
        let rows = stmt.query_map([], |row| row_to_account(row))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn update_last_check_at(&self, id: &str) -> Result<(), AppError> {
        self.conn.execute(
            "UPDATE accounts SET last_check_at=?1 WHERE id=?2",
            params![dt_to_str(&Utc::now()), id],
        )?;
        Ok(())
    }

    pub fn list_distinct_platforms(&self) -> Result<Vec<String>, AppError> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT platform FROM accounts ORDER BY platform")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn delete_account(&self, id: &str) -> Result<(), AppError> {
        self.conn
            .execute("DELETE FROM accounts WHERE id=?1", params![id])?;
        Ok(())
    }

    // ── 设置 ──

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, AppError> {
        self.conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), AppError> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

// ── 辅助函数 ──

fn current_usage_date() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

fn initialize_connection(conn: &Connection) -> Result<(), AppError> {
    configure_connection(conn)?;
    create_core_schema(conn)?;
    run_schema_migrations(conn);
    Ok(())
}

fn configure_connection(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    Ok(())
}

fn create_core_schema(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(CORE_SCHEMA_SQL)?;
    Ok(())
}

fn run_schema_migrations(conn: &Connection) {
    // 增量迁移：给已有数据库添加新列（已存在则忽略）。
    let _ =
        conn.execute_batch("ALTER TABLE accounts ADD COLUMN avatar_url TEXT NOT NULL DEFAULT '';");
    let _ = conn.execute_batch(
        "ALTER TABLE accounts ADD COLUMN task_interval_seconds INTEGER NOT NULL DEFAULT 0;",
    );
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN result_data TEXT;");
}
