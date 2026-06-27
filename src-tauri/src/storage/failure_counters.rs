use chrono::{Duration, Utc};
use rusqlite::{params, OptionalExtension};

use super::helpers::{dt_to_str, str_to_dt};
use super::Storage;
use crate::error::AppError;

impl Storage {
    pub fn record_account_failure(
        &self,
        account_id: &str,
        fingerprint: &str,
        window_seconds: i64,
    ) -> Result<i32, AppError> {
        let now = Utc::now();
        let previous = self
            .conn
            .query_row(
                "SELECT count, window_started_at FROM account_failure_counters
                 WHERE account_id=?1 AND fingerprint=?2",
                params![account_id, fingerprint],
                |row| Ok((row.get::<_, i32>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let count = match previous {
            Some((count, started_at)) if in_window(&started_at, now, window_seconds) => count + 1,
            _ => 1,
        };
        self.conn.execute(
            "INSERT INTO account_failure_counters (
                account_id, fingerprint, count, window_started_at, last_error_at
             ) VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(account_id, fingerprint) DO UPDATE SET
                count=excluded.count,
                window_started_at=excluded.window_started_at,
                last_error_at=excluded.last_error_at",
            params![
                account_id,
                fingerprint,
                count,
                dt_to_str(&now),
                dt_to_str(&now),
            ],
        )?;
        Ok(count)
    }

    pub fn clear_account_failures(&self, account_id: &str) -> Result<(), AppError> {
        self.conn.execute(
            "DELETE FROM account_failure_counters WHERE account_id=?1",
            params![account_id],
        )?;
        Ok(())
    }
}

fn in_window(started_at: &str, now: chrono::DateTime<Utc>, seconds: i64) -> bool {
    str_to_dt(started_at)
        .map(|started| now - started <= Duration::seconds(seconds.max(1)))
        .unwrap_or(false)
}
