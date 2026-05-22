use super::sample_account;
use crate::storage::Storage;
use tempfile::tempdir;

// 回归：数据库里出现非 RFC3339 时间戳时，读取路径必须返回 Err 而不是 panic，
// 否则会 poison 全局 storage Mutex 让整个进程所有后续 lock() 都崩溃。
#[test]
fn corrupt_timestamp_returns_error_not_panic() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.db");
    let storage = Storage::new(&path).unwrap();
    storage
        .insert_account(&sample_account("a1", "doubao"))
        .unwrap();

    // 用独立连接直接把一行的 created_at 改成非法字符串，模拟 DB 内脏数据
    let side = rusqlite::Connection::open(&path).unwrap();
    side.execute(
        "UPDATE accounts SET created_at = 'not-a-timestamp' WHERE id = 'a1'",
        [],
    )
    .unwrap();
    drop(side);

    let result = storage.get_account("a1");
    assert!(result.is_err(), "坏时间戳应返回 Err，而不是 panic");
}

// 回归：login_state 列出现未知字符串时也必须返回 Err，而不是 panic。
#[test]
fn corrupt_enum_returns_error_not_panic() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.db");
    let storage = Storage::new(&path).unwrap();
    storage
        .insert_account(&sample_account("a1", "doubao"))
        .unwrap();

    let side = rusqlite::Connection::open(&path).unwrap();
    side.execute(
        "UPDATE accounts SET login_state = 'not_a_valid_state' WHERE id = 'a1'",
        [],
    )
    .unwrap();
    drop(side);

    let result = storage.get_account("a1");
    assert!(result.is_err(), "未知枚举值应返回 Err，而不是 panic");
}
