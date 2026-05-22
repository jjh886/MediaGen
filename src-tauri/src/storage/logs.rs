use rusqlite::params;

use super::Storage;
use crate::error::AppError;
use crate::types::LogRecord;

impl Storage {
    // ── 日志 ──

    pub fn insert_log(
        &self,
        timestamp: &str,
        module: &str,
        level: &str,
        summary: &str,
        detail: Option<&str>,
        task_id: Option<&str>,
    ) -> Result<(), AppError> {
        self.conn.execute(
            "INSERT INTO logs (timestamp, module, level, summary, detail, task_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![timestamp, module, level, summary, detail, task_id],
        )?;
        Ok(())
    }

    pub fn list_logs(
        &self,
        module: &str,
        level: &str,
        keyword: &str,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<LogRecord>, AppError> {
        let mut conditions = vec!["1=1".to_string()];
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1;

        if !module.is_empty() {
            conditions.push(format!("module = ?{idx}"));
            param_values.push(Box::new(module.to_string()));
            idx += 1;
        }
        if !level.is_empty() {
            conditions.push(format!("level = ?{idx}"));
            param_values.push(Box::new(level.to_string()));
            idx += 1;
        }
        if !keyword.is_empty() {
            conditions.push(format!("summary LIKE ?{idx}"));
            param_values.push(Box::new(format!("%{keyword}%")));
            idx += 1;
        }

        let sql = format!(
            "SELECT id, timestamp, module, level, summary, detail, task_id
             FROM logs WHERE {} ORDER BY id DESC LIMIT ?{} OFFSET ?{}",
            conditions.join(" AND "),
            idx,
            idx + 1,
        );
        param_values.push(Box::new(limit));
        param_values.push(Box::new(offset));

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(LogRecord {
                id: row.get::<_, i64>(0)?.to_string(),
                timestamp: row.get(1)?,
                module: row.get(2)?,
                level: row.get(3)?,
                summary: row.get(4)?,
                detail: row.get(5)?,
                task_id: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn clear_logs(&self) -> Result<u64, AppError> {
        let changed = self.conn.execute("DELETE FROM logs", [])?;
        Ok(changed as u64)
    }

    pub fn cleanup_old_logs(&self, days: i32) -> Result<u64, AppError> {
        let changed = self.conn.execute(
            "DELETE FROM logs WHERE timestamp < datetime('now', ?1)",
            params![format!("-{days} days")],
        )?;
        Ok(changed as u64)
    }
}
