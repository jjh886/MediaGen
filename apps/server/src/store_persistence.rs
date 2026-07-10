use std::path::Path;

use rusqlite::{params, Connection};

use crate::store::StoreSnapshot;

const SNAPSHOT_ID: &str = "current";
const CURRENT_TABLES: &[&str] = &[
    "state_snapshots",
    "api_keys",
    "platform_accounts",
    "proxies",
    "browser_profiles",
    "os_profiles",
    "browser_sessions",
    "verification_sessions",
    "tasks",
    "task_attempts",
    "task_events",
    "account_leases",
    "audit_logs",
    "settings",
    "plugin_installations",
    "model_routes",
];

pub fn read_store_snapshot(path: &Path) -> StoreSnapshot {
    if is_sqlite_path(path) {
        return read_sqlite_snapshot(path).unwrap_or_default();
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<StoreSnapshot>(&raw).ok())
        .unwrap_or_default()
}

pub fn write_store_snapshot(path: &Path, snapshot: &StoreSnapshot) -> Result<(), String> {
    if is_sqlite_path(path) {
        return write_sqlite_snapshot(path, snapshot);
    }
    let raw = serde_json::to_string_pretty(snapshot).map_err(|err| err.to_string())?;
    std::fs::write(path, raw).map_err(|err| err.to_string())
}

fn read_sqlite_snapshot(path: &Path) -> Result<StoreSnapshot, String> {
    let connection = open_database(path)?;
    ensure_schema(&connection)?;
    let raw = connection
        .query_row(
            "SELECT payload FROM state_snapshots WHERE id = ?1",
            params![SNAPSHOT_ID],
            |row| row.get::<_, String>(0),
        )
        .ok();
    Ok(raw
        .and_then(|raw| serde_json::from_str::<StoreSnapshot>(&raw).ok())
        .unwrap_or_default())
}

fn write_sqlite_snapshot(path: &Path, snapshot: &StoreSnapshot) -> Result<(), String> {
    let connection = open_database(path)?;
    ensure_schema(&connection)?;
    let raw = serde_json::to_string(snapshot).map_err(|err| err.to_string())?;
    connection
        .execute(
            "INSERT INTO state_snapshots(id, payload, updated_at)
             VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(id) DO UPDATE SET payload = excluded.payload, updated_at = excluded.updated_at",
            params![SNAPSHOT_ID, raw],
        )
        .map_err(|err| err.to_string())?;
    Ok(())
}

fn open_database(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    Connection::open(path).map_err(|err| err.to_string())
}

fn ensure_schema(connection: &Connection) -> Result<(), String> {
    drop_obsolete_tables(connection)?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS state_snapshots(
                id TEXT PRIMARY KEY,
                payload TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS api_keys(id TEXT PRIMARY KEY, payload TEXT);
            CREATE TABLE IF NOT EXISTS platform_accounts(id TEXT PRIMARY KEY, payload TEXT);
            CREATE TABLE IF NOT EXISTS proxies(id TEXT PRIMARY KEY, payload TEXT);
            CREATE TABLE IF NOT EXISTS browser_profiles(id TEXT PRIMARY KEY, payload TEXT);
            CREATE TABLE IF NOT EXISTS os_profiles(id TEXT PRIMARY KEY, payload TEXT);
            CREATE TABLE IF NOT EXISTS browser_sessions(id TEXT PRIMARY KEY, payload TEXT);
            CREATE TABLE IF NOT EXISTS verification_sessions(id TEXT PRIMARY KEY, payload TEXT);
            CREATE TABLE IF NOT EXISTS tasks(id TEXT PRIMARY KEY, payload TEXT);
            CREATE TABLE IF NOT EXISTS task_attempts(id TEXT PRIMARY KEY, payload TEXT);
            CREATE TABLE IF NOT EXISTS task_events(id TEXT PRIMARY KEY, payload TEXT);
            CREATE TABLE IF NOT EXISTS account_leases(id TEXT PRIMARY KEY, payload TEXT);
            CREATE TABLE IF NOT EXISTS audit_logs(id TEXT PRIMARY KEY, payload TEXT);
            CREATE TABLE IF NOT EXISTS settings(id TEXT PRIMARY KEY, payload TEXT);
            CREATE TABLE IF NOT EXISTS plugin_installations(id TEXT PRIMARY KEY, payload TEXT);
            CREATE TABLE IF NOT EXISTS model_routes(id TEXT PRIMARY KEY, payload TEXT);",
        )
        .map_err(|err| err.to_string())
}

fn drop_obsolete_tables(connection: &Connection) -> Result<(), String> {
    let tables = existing_table_names(connection)?;
    for table in tables {
        if !CURRENT_TABLES.contains(&table.as_str()) {
            // SQLite 不支持参数绑定表名，先只从 sqlite_master 读取表名再做标识符转义。
            connection
                .execute_batch(&format!(
                    "DROP TABLE IF EXISTS \"{}\";",
                    escape_identifier(&table)
                ))
                .map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}

fn existing_table_names(connection: &Connection) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|err| err.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())
}

fn escape_identifier(value: &str) -> String {
    value.replace('"', "\"\"")
}

fn is_sqlite_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| matches!(value, "sqlite" | "sqlite3" | "db"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_snapshot_round_trips() {
        let root = std::env::temp_dir().join("mediagen-sqlite-snapshot-test");
        let _ = std::fs::remove_dir_all(&root);
        let path = root.join("mediagen.sqlite");
        let mut snapshot = StoreSnapshot::default();
        snapshot.settings.api_address = "0.0.0.0".to_string();

        write_store_snapshot(&path, &snapshot).unwrap();
        let loaded = read_store_snapshot(&path);

        assert_eq!(loaded.settings.api_address, "0.0.0.0");
        assert!(path.exists());
    }

    #[test]
    fn sqlite_schema_drops_obsolete_tables() {
        let root = std::env::temp_dir().join("mediagen-sqlite-schema-clean-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("mediagen.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("CREATE TABLE stale_feature(id TEXT PRIMARY KEY);")
            .unwrap();
        drop(connection);

        write_store_snapshot(&path, &StoreSnapshot::default()).unwrap();
        let tables = existing_table_names(&Connection::open(&path).unwrap()).unwrap();

        assert!(tables.contains(&"state_snapshots".to_string()));
        assert!(!tables.contains(&"stale_feature".to_string()));
    }

    #[test]
    fn json_snapshot_round_trips_and_missing_file_defaults() {
        let root = std::env::temp_dir().join("mediagen-json-snapshot-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("state.json");
        let mut snapshot = StoreSnapshot::default();
        snapshot.settings.api_port = 19080;

        assert_eq!(read_store_snapshot(&path).settings.api_port, 18080);
        write_store_snapshot(&path, &snapshot).unwrap();

        assert_eq!(read_store_snapshot(&path).settings.api_port, 19080);
    }
}
