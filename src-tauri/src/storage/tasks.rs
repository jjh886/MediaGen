use chrono::Utc;
use rusqlite::{params, OptionalExtension};

use super::helpers::{dt_to_str, enum_to_str, row_to_task};
use super::Storage;
use crate::error::AppError;
use crate::types::*;

impl Storage {
    // ── 任务 ──

    pub fn insert_task(&self, t: &TaskRecord) -> Result<(), AppError> {
        self.conn.execute(
            "INSERT INTO tasks (
                id, platform, task_kind, model_id, prompt, account_id,
                status, result_url, storage_key, error_message, result_data,
                created_at, completed_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                t.id,
                t.platform,
                enum_to_str(&t.task_kind),
                t.model_id,
                t.prompt,
                t.account_id,
                enum_to_str(&t.status),
                t.result_url,
                t.storage_key,
                t.error_message,
                t.result_data,
                dt_to_str(&t.created_at),
                t.completed_at.map(|d| dt_to_str(&d)),
            ],
        )?;
        Ok(())
    }

    pub fn get_task(&self, id: &str) -> Result<Option<TaskRecord>, AppError> {
        self.conn
            .query_row("SELECT * FROM tasks WHERE id=?1", params![id], |row| {
                row_to_task(row)
            })
            .optional()
            .map_err(AppError::from)
    }

    pub fn list_tasks(
        &self,
        platform: &str,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<TaskRecord>, AppError> {
        let (sql, params_vec): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = if platform.is_empty()
        {
            (
                "SELECT * FROM tasks ORDER BY created_at DESC, id ASC LIMIT ?1 OFFSET ?2",
                vec![Box::new(limit), Box::new(offset)],
            )
        } else {
            (
                "SELECT * FROM tasks WHERE platform = ?1 ORDER BY created_at DESC, id ASC LIMIT ?2 OFFSET ?3",
                vec![Box::new(platform.to_string()), Box::new(limit), Box::new(offset)],
            )
        };
        let mut stmt = self.conn.prepare(sql)?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| row_to_task(row))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn update_task_status(
        &self,
        id: &str,
        status: TaskStatus,
        result_url: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), AppError> {
        let completed_at = if status == TaskStatus::Completed || status == TaskStatus::Failed {
            Some(dt_to_str(&Utc::now()))
        } else {
            None
        };
        self.conn.execute(
            "UPDATE tasks SET status=?1, result_url=?2, error_message=?3, completed_at=?4
             WHERE id=?5",
            params![enum_to_str(&status), result_url, error, completed_at, id],
        )?;
        Ok(())
    }

    pub fn update_task_result_data(&self, id: &str, result_data: &str) -> Result<(), AppError> {
        self.conn.execute(
            "UPDATE tasks SET result_data=?1 WHERE id=?2",
            params![result_data, id],
        )?;
        Ok(())
    }
}
