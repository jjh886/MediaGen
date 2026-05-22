//! 异步账号验证任务
//!
//! 当豆包等浏览器平台生成任务返回 `rate limited` 时，主流程已经把账号置为 cooldown、
//! 失败返回给调用方；本模块在此基础上额外 spawn 一个独立 tokio 任务：
//!
//! 1. 克隆该任务的 flow，把 `browser.open` 改为
//!    `headless=false + force_restart=true + offscreen=true`，并注入一个固定的
//!    `session_id`，让 watchdog 也能通过该 id 访问浏览器 tab。
//!    `offscreen=true` 会给 Chrome 追加 `--window-position=-32000,-32000`，窗口启动后
//!    在屏幕外，用户看不到，避免打扰。
//! 2. 以原任务参数重跑 flow。
//! 3. Captcha Watchdog：每秒轮询 `.vc-captcha-verify-visibility` 元素：
//!    - 首次检测到 → 调用 `browser.show_window` 把窗口移回可见区域，等用户处理。
//!    - 出现过后又消失 → 延迟随机 3-5 秒自动关闭浏览器。
//! 4. flow 完成或失败（包括用户/watchdog 关闭窗口导致 tab 失效）→ 账号恢复 Available
//! 5. 10 分钟仍未结束 → 账号标 VerificationRequired 并 kill 浏览器进程
//!
//! 为防止"套娃"（验证任务自己又触发一次验证），spawn 时会在 `initial_context`
//! 插入 `is_verification_task: true` 标记，主流程判定后不再 spawn。

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::api_server::ImageGenContext;
use crate::engine::browser_engine::kill_chrome_by_profile;
use crate::orchestrator::Orchestrator;
use crate::sync_ext::MutexExt;
use crate::types::{FlowDef, LoginState};
use crate::AppState;

const VERIFICATION_TIMEOUT_SECS: u64 = 600;
const CAPTCHA_SELECTOR: &str = ".vc-captcha-verify-visibility";
/// Watchdog 轮询间隔
const WATCHDOG_INTERVAL_MS: u64 = 1_000;
/// 验证码消失后随机延迟下限（毫秒）
const CAPTCHA_CLEARED_MIN_MS: u64 = 3_000;
/// 验证码消失后随机延迟上限（毫秒）
const CAPTCHA_CLEARED_MAX_MS: u64 = 5_000;

/// 检查 ctx 是否本身就是一个有头验证任务（防止套娃触发无限次 spawn）。
pub(crate) fn is_verification_task(ctx: &ImageGenContext) -> bool {
    ctx.initial_context
        .get("is_verification_task")
        .and_then(|v| v.as_bool())
        == Some(true)
}

/// 判断错误字符串是否属于"账号被平台限流/要求人工验证"类，用于决定是否 spawn 验证任务。
pub(crate) fn error_suggests_verification(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    let explicit_cooldown = lower.contains("rate_limit_exceeded")
        || lower.contains("you're generating images too quickly")
        || lower.contains("you’re generating images too quickly")
        || lower.contains("please wait for")
        || lower.contains("生成过于频繁");
    if explicit_cooldown {
        return false;
    }
    lower.contains("rate limited") || lower.contains("rate limit") || lower.contains("需要验证")
}

/// 异步 spawn 一个人工验证任务，不阻塞调用方。
pub(crate) fn spawn(state: Arc<AppState>, ctx: ImageGenContext) {
    tokio::spawn(async move {
        run(state, ctx).await;
    });
}

async fn run(state: Arc<AppState>, mut ctx: ImageGenContext) {
    let account_id = ctx.account_id.clone();
    let profile_dir = ctx.profile_dir.clone();

    // 预生成 session_id，让 flow 里的 browser.open 和 watchdog 共享同一个 tab
    let session_id = uuid::Uuid::new_v4().to_string();

    tracing::warn!(
        account_id,
        profile_dir,
        platform = %ctx.platform,
        session_id = %session_id,
        "启动有头浏览器进行人工验证（最长 {} 秒）",
        VERIFICATION_TIMEOUT_SECS
    );

    let (visible_flow, scripts_dir_override) = build_visible_flow(&state, &ctx, &session_id);

    if let Value::Object(ref mut map) = ctx.initial_context {
        map.insert("is_verification_task".to_string(), json!(true));
        if let Some(sd) = scripts_dir_override.as_ref() {
            // 走跨插件 fallback 时，脚本要从 fallback 插件的目录加载
            map.insert("scripts_dir".to_string(), json!(sd));
        }
    }

    let flow_fut = Orchestrator::execute_flow(
        &visible_flow,
        &state.engine_registry,
        ctx.initial_context.clone(),
    );
    let watchdog_fut = watch_captcha(state.clone(), session_id.clone());

    let outcome = tokio::select! {
        // flow 先结束（成功 / 失败 / 窗口被 watchdog 关掉导致失败）
        res = flow_fut => FlowOutcome::Flow(res),
        // watchdog 主动关了浏览器
        _ = watchdog_fut => FlowOutcome::WatchdogClosed,
        // 整体 10 分钟超时
        _ = tokio::time::sleep(Duration::from_secs(VERIFICATION_TIMEOUT_SECS)) => FlowOutcome::Timeout,
    };

    match outcome {
        FlowOutcome::Flow(Ok(_)) => {
            tracing::info!(account_id, "人工验证 flow 正常完成，账号恢复可用");
            set_account_state(&state, &account_id, LoginState::Available, "人工验证完成");
        }
        FlowOutcome::Flow(Err(e)) => {
            // flow 失败（包括用户关闭窗口导致 tab 失效）视作人已介入，账号恢复
            tracing::info!(
                account_id,
                error = %e,
                "人工验证 flow 结束（可能窗口被关闭），账号恢复可用"
            );
            set_account_state(
                &state,
                &account_id,
                LoginState::Available,
                "人工验证流程结束",
            );
        }
        FlowOutcome::WatchdogClosed => {
            tracing::info!(
                account_id,
                "检测到验证码消失并已自动关闭浏览器，账号恢复可用"
            );
            set_account_state(
                &state,
                &account_id,
                LoginState::Available,
                "人工验证完成（自动关窗）",
            );
        }
        FlowOutcome::Timeout => {
            tracing::warn!(
                account_id,
                profile_dir,
                "人工验证 {} 秒超时，账号标记为需要验证",
                VERIFICATION_TIMEOUT_SECS
            );
            set_account_state(
                &state,
                &account_id,
                LoginState::VerificationRequired,
                "人工验证超时",
            );
            kill_chrome_by_profile(&profile_dir);
        }
    }
}

enum FlowOutcome {
    Flow(Result<Vec<crate::types::StepResult>, crate::error::AppError>),
    WatchdogClosed,
    Timeout,
}

/// Watchdog：轮询验证码 DOM 元素。元素先出现过、后消失 → 随机 3-5 秒后关浏览器退出。
/// 若元素从未出现，watchdog 不主动干预，靠 flow 自身或 timeout 结束。
async fn watch_captcha(state: Arc<AppState>, session_id: String) {
    let mut seen = false;
    let probe_script = format!(
        "JSON.stringify({{ present: document.querySelector('{}') !== null }})",
        CAPTCHA_SELECTOR
    );

    loop {
        tokio::time::sleep(Duration::from_millis(WATCHDOG_INTERVAL_MS)).await;

        let present = match probe_captcha_present(&state, &session_id, &probe_script).await {
            Some(v) => v,
            None => continue, // tab 尚未就绪或临时失败，下一轮再试
        };

        if present {
            if !seen {
                tracing::info!(
                    session_id = %session_id,
                    "检测到验证码元素，将窗口移到可见区域等待用户处理"
                );
                let show_result = state
                    .engine_registry
                    .execute("browser.show_window", &json!({ "session_id": session_id }))
                    .await;
                if let Err(e) = show_result {
                    tracing::warn!(session_id = %session_id, error = %e, "拉回窗口失败");
                }
                seen = true;
            }
            continue;
        }

        if seen {
            let delay = random_between_ms(CAPTCHA_CLEARED_MIN_MS, CAPTCHA_CLEARED_MAX_MS);
            tracing::info!(
                session_id = %session_id,
                delay_ms = delay,
                "验证码元素已消失，{}ms 后自动关闭浏览器",
                delay
            );
            tokio::time::sleep(Duration::from_millis(delay)).await;
            let close_result = state
                .engine_registry
                .execute(
                    "browser.close",
                    &json!({ "session_id": session_id, "force": true }),
                )
                .await;
            if let Err(e) = close_result {
                tracing::warn!(session_id = %session_id, error = %e, "自动关闭浏览器失败");
            }
            return;
        }
    }
}

/// 返回 Some(true/false) 表示验证码是否可见；None 表示探测失败（tab 未就绪等）。
async fn probe_captcha_present(state: &AppState, session_id: &str, script: &str) -> Option<bool> {
    let result = state
        .engine_registry
        .execute(
            "browser.run_script",
            &json!({
                "session_id": session_id,
                "script": script,
                "await_promise": false,
            }),
        )
        .await
        .ok()?;
    if !result.success {
        return None;
    }
    result.data.get("present").and_then(|v| v.as_bool())
}

/// 克隆 flow：首个 `browser.open` step 改为 headless=false + force_restart=true + offscreen=true
/// 并注入指定的 `session_id`，让外部 watchdog 能通过该 id 访问 tab。
///
/// `offscreen=true` 让窗口以 `--window-position=-32000,-32000` 启动在屏幕外，
/// 用户无感知；watchdog 检测到验证码时再调用 `browser.show_window` 拉回可见区域。
fn clone_flow_headed(flow: &FlowDef, session_id: &str) -> FlowDef {
    let mut flow = flow.clone();
    for step in flow.steps.iter_mut() {
        if step.action == "browser.open" {
            if let Value::Object(ref mut map) = step.params {
                map.insert("headless".to_string(), json!(false));
                map.insert("force_restart".to_string(), json!(true));
                map.insert("offscreen".to_string(), json!(true));
                map.insert("session_id".to_string(), json!(session_id));
            }
            break;
        }
    }
    flow
}

/// 选择有头验证 flow。
/// 优先级：1) 当前平台 manifest 的 `verification_fallback`（跨插件 DOM flow）
///          2) 回退到当前 flow 自身 clone。
///
/// 返回 `(flow, scripts_dir_override)`。走跨插件 fallback 时 scripts_dir_override = Some(...)，
/// 调用方需把这个值注入 initial_context.scripts_dir，让 `script_file` 从目标插件的 scripts 目录加载。
fn build_visible_flow(
    state: &AppState,
    ctx: &ImageGenContext,
    session_id: &str,
) -> (FlowDef, Option<String>) {
    let manifest = match state.plugin_loader.get_plugin(&ctx.platform) {
        Some(m) => m,
        None => return (clone_flow_headed(&ctx.flow, session_id), None),
    };
    let vf = match manifest.verification_fallback.as_ref() {
        Some(v) => v,
        None => return (clone_flow_headed(&ctx.flow, session_id), None),
    };
    let fb_manifest = match state.plugin_loader.get_plugin(&vf.platform) {
        Some(m) => m,
        None => {
            tracing::warn!(platform = %vf.platform,
                "verification_fallback 指向的平台未加载，退回自身 flow");
            return (clone_flow_headed(&ctx.flow, session_id), None);
        }
    };
    let flow = match fb_manifest.flows.get(&vf.flow) {
        Some(f) => f,
        None => {
            tracing::warn!(platform = %vf.platform, flow = %vf.flow,
                "verification_fallback 指向的 flow 不存在，退回自身 flow");
            return (clone_flow_headed(&ctx.flow, session_id), None);
        }
    };
    let scripts_dir = state
        .plugin_loader
        .get_plugin_dir(&vf.platform)
        .map(|d| d.join("scripts").to_string_lossy().to_string());
    tracing::info!(
        from = %ctx.platform,
        to = %vf.platform,
        flow_name = %vf.flow,
        scripts_dir = ?scripts_dir,
        "使用跨插件 verification_fallback flow 进行有头人工验证"
    );
    (clone_flow_headed(flow, session_id), scripts_dir)
}

fn should_preserve_current_state_when_restoring_available(
    current_state: &LoginState,
    cooldown_until: Option<chrono::DateTime<chrono::Utc>>,
) -> bool {
    matches!(current_state, LoginState::Cooldown)
        && cooldown_until
            .map(|t| t > chrono::Utc::now())
            .unwrap_or(false)
}

fn set_account_state(state: &AppState, account_id: &str, login_state: LoginState, reason: &str) {
    let storage = state.storage.lock_recover();
    if matches!(login_state, LoginState::Available) {
        if let Ok(Some(account)) = storage.get_account(account_id) {
            if should_preserve_current_state_when_restoring_available(
                &account.login_state,
                account.cooldown_until,
            ) {
                tracing::info!(
                    account_id,
                    current_state = ?account.login_state,
                    cooldown_until = ?account.cooldown_until,
                    "当前账号仍在冷却中，跳过人工验证后的可用状态回写"
                );
                return;
            }
        }
    }
    if let Err(e) = storage.update_login_state_with_cooldown(account_id, login_state, reason, None)
    {
        tracing::error!(account_id, error = %e, "更新账号状态失败");
    }
}

/// 基于系统时间纳秒的简单伪随机，避免额外引入 rand 依赖。范围 [min_ms, max_ms]。
fn random_between_ms(min_ms: u64, max_ms: u64) -> u64 {
    if max_ms <= min_ms {
        return min_ms;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let span = max_ms - min_ms + 1;
    min_ms + (nanos % span)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_suggests_verification() {
        assert!(error_suggests_verification("平台错误: rate limited"));
        assert!(error_suggests_verification("RATE LIMIT"));
        assert!(error_suggests_verification("需要验证"));
        assert!(!error_suggests_verification(
            "平台错误: 生成过于频繁: You're generating images too quickly. Please wait for 8 minutes before generating more images."
        ));
        assert!(!error_suggests_verification("rate_limit_exceeded"));
        assert!(!error_suggests_verification("内容违规"));
        assert!(!error_suggests_verification("登录失效"));
    }

    #[test]
    fn test_preserve_cooldown_when_restoring_available() {
        let future = chrono::Utc::now() + chrono::Duration::minutes(8);
        assert!(should_preserve_current_state_when_restoring_available(
            &LoginState::Cooldown,
            Some(future),
        ));
        assert!(!should_preserve_current_state_when_restoring_available(
            &LoginState::Cooldown,
            Some(chrono::Utc::now() - chrono::Duration::seconds(1)),
        ));
        assert!(!should_preserve_current_state_when_restoring_available(
            &LoginState::Available,
            Some(future),
        ));
    }

    #[test]
    fn test_clone_flow_headed_overrides_open() {
        use crate::types::StepDef;
        let flow = FlowDef {
            steps: vec![
                StepDef {
                    action: "browser.open".to_string(),
                    params: json!({ "headless": true, "profile_dir": "/tmp" }),
                },
                StepDef {
                    action: "browser.navigate".to_string(),
                    params: json!({ "url": "https://example.com" }),
                },
            ],
        };
        let headed = clone_flow_headed(&flow, "sid-123");
        assert_eq!(headed.steps[0].params["headless"], json!(false));
        assert_eq!(headed.steps[0].params["force_restart"], json!(true));
        assert_eq!(headed.steps[0].params["offscreen"], json!(true));
        assert_eq!(headed.steps[0].params["session_id"], "sid-123");
        assert_eq!(headed.steps[0].params["profile_dir"], "/tmp");
        assert_eq!(headed.steps[1].params["url"], "https://example.com");
    }

    #[test]
    fn test_random_between_ms_within_range() {
        for _ in 0..50 {
            let v = random_between_ms(3000, 5000);
            assert!(v >= 3000 && v <= 5000, "值 {} 超出范围", v);
            std::thread::sleep(std::time::Duration::from_micros(1));
        }
    }
}
