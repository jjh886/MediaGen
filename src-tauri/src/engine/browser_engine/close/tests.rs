//! `browser.close` 相关的单元测试。从 `close.rs` 拆出，控制单文件行数。

use super::super::BrowserEngine;
use serde_json::json;

/// 回归：未给 session_id 也未给 profile_dir 时 browser.close 必须返回错误，
/// 防止调用方静默失败导致浏览器进程泄漏。
#[tokio::test]
async fn close_without_any_identifier_errors() {
    let engine = BrowserEngine::new();
    let err = engine.close(&json!({})).await.unwrap_err();
    assert!(
        err.to_string().contains("session_id") && err.to_string().contains("profile_dir"),
        "错误信息应提示两个参数都缺失，实际: {err}"
    );
}

/// 回归：按 profile_dir 关闭在引擎内部没有任何状态时必须安全返回（no-op），
/// 以便任务结束的兜底清理对 API-only/未启动浏览器的平台也能无副作用地调用。
#[tokio::test]
async fn close_by_profile_is_noop_when_no_state() {
    let engine = BrowserEngine::new();
    let result = engine
        .close(&json!({ "profile_dir": "/tmp/nonexistent-mediagen-test" }))
        .await
        .expect("close 应成功");
    assert_eq!(result.data["closed"], json!(true));
    assert_eq!(result.data["cookies_json"], json!(""));
    assert_eq!(
        result.data["profile_dir"],
        json!("/tmp/nonexistent-mediagen-test")
    );
}

/// 回归：空 engine 下 sessions_count_for_profile 必须返回 0，
/// 确保 close 非 force 分支在没有 session 时不会把 `same_profile_count` 算成 >1
/// 而误走"关闭临时 tab"路径。
#[test]
fn sessions_count_for_profile_empty_engine_returns_zero() {
    let engine = BrowserEngine::new();
    assert_eq!(engine.sessions_count_for_profile("/tmp/any"), 0);
    assert_eq!(engine.sessions_count_for_profile(""), 0);
}

/// 回归：多 tab 并发下 close 的并发判断逻辑——排除调用方自己后，必须还能
/// 识别出"同 profile 下其他 in_use=true 的 session"。
///
/// 直接模拟 bug 场景（2026-04 的豆包并发崩溃）：
///   profile /p1 下：session A（当前调用方）、session B（正在跑 mouseMoved）
///   → A 走 close 时 count_other_active_sessions 必须返回 1，
///   否则上层判定"无其他活跃任务"就会走 gracefully_close_profile → SIGTERM → B 崩。
#[test]
fn count_other_active_excludes_self_and_counts_concurrent_sessions() {
    use super::count_other_active_sessions;
    let entries = [
        ("A", "/p1", true),  // 调用方自己（排除）
        ("B", "/p1", true),  // 同 profile 的并发任务
        ("C", "/p1", false), // 同 profile 但已空闲（不计）
        ("D", "/p2", true),  // 别的 profile（不计）
    ];
    assert_eq!(
        count_other_active_sessions(entries.iter().map(|(s, p, i)| (*s, *p, *i)), "A", "/p1"),
        1,
        "必须识别出 B 是同 profile 下另一个活跃任务"
    );
}

/// 回归：profile 下只有自己一个 in_use session 时，排除自己后 count=0，
/// 上层据此判定"我是最后一个"，应走 graceful Chrome 关闭。
#[test]
fn count_other_active_returns_zero_when_self_is_last() {
    use super::count_other_active_sessions;
    let entries = [
        ("A", "/p1", true),
        ("B", "/p1", false), // 空闲不算
    ];
    assert_eq!(
        count_other_active_sessions(entries.iter().map(|(s, p, i)| (*s, *p, *i)), "A", "/p1"),
        0
    );
}

/// 回归：engine 空状态下 close(session_id + profile_dir) 不应 panic，
/// 应视作幂等 no-op（真实场景：post_flow_cleanup 兜底调 close 时 flow 里已经关过）。
#[tokio::test]
async fn close_with_session_and_profile_is_safe_when_no_state() {
    let engine = BrowserEngine::new();
    let result = engine
        .close(&json!({
            "session_id": "unknown-session",
            "profile_dir": "/tmp/nonexistent-mediagen-test",
        }))
        .await
        .expect("空状态 close 必须幂等返回 Ok");
    assert!(result.success);
}

/// 回归：pid_is_alive 对一个已退出的 PID 必须返回 false，
/// 避免优雅关闭后的等待循环无限转圈。
#[test]
fn pid_is_alive_returns_false_for_exited_process() {
    let mut child = std::process::Command::new("true")
        .spawn()
        .expect("spawn true");
    let pid = child.id();
    let _ = child.wait();
    assert!(
        !super::super::process::pid_is_alive(pid),
        "已退出进程 pid={pid} 不应被判为存活"
    );
}

/// 回归（2026-04-20）：Chrome 响应 SIGTERM 后变成 zombie（headless_chrome 只在
/// Browser Drop 时才 wait()），`kill -0` 此时返回成功导致轮询始终判活、5 秒
/// 超时触发 `Browser::Drop` 最终 SIGKILL 路径。pid_is_alive 必须把 zombie
/// 视为"已退出"，否则优雅关闭等同于强杀，Google 等站点的登录态会丢失。
#[cfg(unix)]
#[test]
fn pid_is_alive_returns_false_for_zombie_process() {
    // 用 Command::spawn 起一个立即退出的子进程，但故意不 wait —— 子进程会停在
    // zombie 状态，直到本测试结束时 Child drop。
    let child = std::process::Command::new("true")
        .spawn()
        .expect("spawn true");
    let pid = child.id();
    // 给内核一点时间把子进程从 Running 切换到 Zombie（状态切换非常快，但不是 0 延迟）
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        let out = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "state="])
            .output()
            .expect("ps 执行失败");
        let state = std::str::from_utf8(&out.stdout).unwrap_or("").trim();
        if state.starts_with('Z') {
            break;
        }
    }
    assert!(
        !super::super::process::pid_is_alive(pid),
        "zombie 进程 pid={pid} 不应被判为存活"
    );
    // Child 在作用域结束时自动 Drop 并 reap，不会泄漏
    drop(child);
}
