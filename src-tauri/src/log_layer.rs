//! 自定义 tracing Layer：将日志事件写入 SQLite logs 表。
//! 使用独立的 Connection（WAL 模式支持并发读写）。

use std::sync::Mutex;

use rusqlite::{params, Connection};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

pub struct SqliteLogLayer {
    conn: Mutex<Connection>,
}

impl SqliteLogLayer {
    pub fn new(db_path: &std::path::Path) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

impl<S: Subscriber> Layer<S> for SqliteLogLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let target = meta.target();

        // 只捕获 MediaGen 自身模块的日志，避免第三方库日志把数据库刷爆。
        if !target.starts_with("mediagen") {
            return;
        }

        let level = match *meta.level() {
            tracing::Level::ERROR => "error",
            tracing::Level::WARN => "warn",
            tracing::Level::INFO => "info",
            tracing::Level::DEBUG | tracing::Level::TRACE => "debug",
        };

        // 将 Rust module path 映射为前端模块类别
        let module = map_module(target);

        // 收集字段
        let mut visitor = LogVisitor::default();
        event.record(&mut visitor);

        let summary = visitor.message.unwrap_or_default();
        if summary.is_empty() {
            return;
        }

        let detail = if visitor.fields.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&visitor.fields).unwrap_or_default())
        };

        let timestamp = chrono::Utc::now()
            .format("%Y-%m-%d %H:%M:%S%.3f")
            .to_string();

        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "INSERT INTO logs (timestamp, module, level, summary, detail, task_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![timestamp, module, level, summary, detail, visitor.task_id],
            );
        }
    }
}

fn map_module(target: &str) -> &'static str {
    if target.contains("engine") {
        "engine"
    } else if target.contains("scheduler") {
        "scheduler"
    } else if target.contains("account") || target.contains("login") {
        "account"
    } else if target.contains("api_server") {
        "api"
    } else if target.contains("registration") || target.contains("register") {
        "registration"
    } else {
        "engine"
    }
}

#[derive(Default)]
struct LogVisitor {
    message: Option<String>,
    task_id: Option<String>,
    fields: serde_json::Map<String, serde_json::Value>,
}

impl Visit for LogVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let name = field.name();
        let val = format!("{:?}", value);
        if name == "message" {
            self.message = Some(val);
        } else if name == "task_id" {
            self.task_id = Some(val.trim_matches('"').to_string());
        } else {
            self.fields
                .insert(name.to_string(), serde_json::Value::String(val));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let name = field.name();
        if name == "message" {
            self.message = Some(value.to_string());
        } else if name == "task_id" {
            self.task_id = Some(value.to_string());
        } else {
            self.fields.insert(
                name.to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::Number(value.into()),
        );
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::Number(value.into()),
        );
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::Bool(value));
    }
}
