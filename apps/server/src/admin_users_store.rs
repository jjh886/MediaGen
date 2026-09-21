use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path, sync::Mutex};

pub(crate) struct AdminStore {
    pub enabled: bool,
    pub initialized_from_env: bool,
    pub db: Mutex<Connection>,
    pub attempts: Mutex<HashMap<String, (u32, i64)>>,
    pub hashing: tokio::sync::Semaphore,
}
#[derive(Clone, Serialize, Debug)]
pub(crate) struct AdminUser {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_login_at: Option<String>,
}
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct UserUpdate {
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub enabled: Option<bool>,
}
fn user(row: &rusqlite::Row<'_>) -> rusqlite::Result<AdminUser> {
    Ok(AdminUser {
        id: row.get(0)?,
        username: row.get(1)?,
        display_name: row.get(2)?,
        enabled: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        last_login_at: row.get(6)?,
    })
}
const COLUMNS: &str = "id,username,display_name,enabled,created_at,updated_at,last_login_at";
fn storage_error(_: rusqlite::Error) -> String {
    "管理员数据保存或读取失败".into()
}
impl AdminStore {
    pub fn open(path: &Path, enabled: bool) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| "无法创建管理员数据目录")?;
        }
        let db = Connection::open(path).map_err(storage_error)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| "无法设置管理员数据权限")?;
        }
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(storage_error)?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS admin_users(id TEXT PRIMARY KEY,username TEXT NOT NULL COLLATE NOCASE UNIQUE,display_name TEXT NOT NULL,password_hash TEXT NOT NULL,enabled INTEGER NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL,last_login_at TEXT);
            CREATE TABLE IF NOT EXISTS admin_sessions(token_hash TEXT PRIMARY KEY,user_id TEXT NOT NULL REFERENCES admin_users(id) ON DELETE CASCADE,expires_at INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS admin_sessions_user ON admin_sessions(user_id);").map_err(storage_error)?;
        Ok(Self {
            enabled,
            initialized_from_env: false,
            db: Mutex::new(db),
            attempts: Mutex::default(),
            hashing: tokio::sync::Semaphore::new(4),
        })
    }
    pub fn configured(&self) -> bool {
        self.list().map(|users| !users.is_empty()).unwrap_or(true)
    }
    pub fn list(&self) -> Result<Vec<AdminUser>, String> {
        let db = self.db.lock().map_err(|_| "管理员存储不可用")?;
        let mut stmt = db
            .prepare(&format!(
                "SELECT {COLUMNS} FROM admin_users ORDER BY created_at,id"
            ))
            .map_err(storage_error)?;
        let result = stmt
            .query_map([], user)
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error);
        result
    }
    pub fn credential(&self, name: &str) -> Result<Option<(AdminUser, String)>, String> {
        self.db
            .lock()
            .map_err(|_| "管理员存储不可用")?
            .query_row(
                &format!("SELECT {COLUMNS},password_hash FROM admin_users WHERE username=?1"),
                [name],
                |row| Ok((user(row)?, row.get(7)?)),
            )
            .optional()
            .map_err(storage_error)
    }
    pub fn create(&self, name: &str, display: &str, hash: &str) -> Result<AdminUser, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let db = self.db.lock().map_err(|_| "管理员存储不可用")?;
        if db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM admin_users WHERE username=?1)",
                [name],
                |r| r.get::<_, bool>(0),
            )
            .map_err(storage_error)?
        {
            return Err("用户名已存在".into());
        }
        db.execute("INSERT INTO admin_users(id,username,display_name,password_hash,enabled,created_at,updated_at) VALUES(?1,?2,?3,?4,1,?5,?5)",params![id,name,display,hash,now]).map_err(storage_error)?;
        Ok(AdminUser {
            id,
            username: name.into(),
            display_name: display.into(),
            enabled: true,
            created_at: now.clone(),
            updated_at: now,
            last_login_at: None,
        })
    }
    pub fn update(
        &self,
        id: &str,
        update: UserUpdate,
        actor: Option<&str>,
    ) -> Result<AdminUser, String> {
        let mut db = self.db.lock().map_err(|_| "管理员存储不可用")?;
        let tx = db.transaction().map_err(storage_error)?;
        let mut old = tx
            .query_row(
                &format!("SELECT {COLUMNS} FROM admin_users WHERE id=?1"),
                [id],
                user,
            )
            .optional()
            .map_err(storage_error)?
            .ok_or("用户不存在")?;
        let enabled = update.enabled.unwrap_or(old.enabled);
        if old.enabled && !enabled {
            protect_last(&tx, id, actor)?;
        }
        let name = update.username.unwrap_or(old.username.clone());
        let display = update.display_name.unwrap_or(old.display_name.clone());
        if tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM admin_users WHERE username=?1 AND id<>?2)",
                params![name, id],
                |r| r.get::<_, bool>(0),
            )
            .map_err(storage_error)?
        {
            return Err("用户名已存在".into());
        }
        let now = Utc::now().to_rfc3339();
        tx.execute("UPDATE admin_users SET username=?1,display_name=?2,enabled=?3,updated_at=?4 WHERE id=?5",params![name,display,enabled,now,id]).map_err(storage_error)?;
        if !enabled || name != old.username {
            tx.execute("DELETE FROM admin_sessions WHERE user_id=?1", [id])
                .map_err(storage_error)?;
        }
        tx.commit().map_err(storage_error)?;
        old.username = name;
        old.display_name = display;
        old.enabled = enabled;
        old.updated_at = now;
        Ok(old)
    }
    pub fn delete(&self, id: &str, actor: Option<&str>) -> Result<(), String> {
        let mut db = self.db.lock().map_err(|_| "管理员存储不可用")?;
        let tx = db.transaction().map_err(storage_error)?;
        let enabled = tx
            .query_row("SELECT enabled FROM admin_users WHERE id=?1", [id], |r| {
                r.get::<_, bool>(0)
            })
            .optional()
            .map_err(storage_error)?
            .ok_or("用户不存在")?;
        if enabled {
            protect_last(&tx, id, actor)?;
        }
        tx.execute("DELETE FROM admin_users WHERE id=?1", [id])
            .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(())
    }
    pub fn password(&self, id: &str, hash: &str) -> Result<(), String> {
        self.replace_password(id, hash, None)
    }
    pub fn replace_password(
        &self,
        id: &str,
        hash: &str,
        expected: Option<&str>,
    ) -> Result<(), String> {
        let mut db = self.db.lock().map_err(|_| "管理员存储不可用")?;
        let tx = db.transaction().map_err(storage_error)?;
        if tx
            .execute(
                "UPDATE admin_users SET password_hash=?1,updated_at=?2 WHERE id=?3 AND (?4 IS NULL OR password_hash=?4)",
                params![hash, Utc::now().to_rfc3339(), id, expected],
            )
            .map_err(storage_error)?
            == 0
        {
            return Err("用户不存在".into());
        }
        tx.execute("DELETE FROM admin_sessions WHERE user_id=?1", [id])
            .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(())
    }
    pub fn session(&self, token: &str) -> Option<AdminUser> {
        let hash = crate::access_control::hash_secret(token);
        self.db.lock().ok()?.query_row(&format!("SELECT {} FROM admin_users u JOIN admin_sessions s ON u.id=s.user_id WHERE s.token_hash=?1 AND s.expires_at>?2 AND u.enabled=1", COLUMNS.split(',').map(|c|format!("u.{c}")).collect::<Vec<_>>().join(",")),params![hash,Utc::now().timestamp()],user).optional().ok().flatten()
    }
    pub fn issue_session(&self, id: &str, expected_hash: &str, token: &str) -> Result<(), String> {
        let mut db = self.db.lock().map_err(|_| "管理员存储不可用")?;
        let tx = db.transaction().map_err(storage_error)?;
        let valid=tx.query_row("SELECT EXISTS(SELECT 1 FROM admin_users WHERE id=?1 AND password_hash=?2 AND enabled=1)",params![id,expected_hash],|r|r.get::<_,bool>(0)).map_err(storage_error)?;
        if !valid {
            return Err("账号状态已变化，请重新登录".into());
        }
        let now = Utc::now();
        tx.execute(
            "DELETE FROM admin_sessions WHERE expires_at<=?1",
            [now.timestamp()],
        )
        .map_err(storage_error)?;
        tx.execute(
            "INSERT INTO admin_sessions VALUES(?1,?2,?3)",
            params![
                crate::access_control::hash_secret(token),
                id,
                now.timestamp() + 43200
            ],
        )
        .map_err(storage_error)?;
        tx.execute(
            "UPDATE admin_users SET last_login_at=?1 WHERE id=?2",
            params![now.to_rfc3339(), id],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(())
    }
    pub fn logout(&self, token: &str) {
        if let Ok(db) = self.db.lock() {
            let _ = db.execute(
                "DELETE FROM admin_sessions WHERE token_hash=?1",
                [crate::access_control::hash_secret(token)],
            );
        }
    }
}
fn protect_last(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
    actor: Option<&str>,
) -> Result<(), String> {
    if actor == Some(id) {
        return Err("不能删除或停用当前登录用户".into());
    }
    let count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM admin_users WHERE enabled=1",
            [],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if count <= 1 {
        return Err("必须保留至少一个启用的管理员".into());
    }
    Ok(())
}
