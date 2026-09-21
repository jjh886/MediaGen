use actix_web::web;
use mediagen_core::{ModelDef, PluginManifest, TaskKind};
use serde_json::Value;

use crate::failure_strategy::{classify_failure, FailureDecision};
use crate::image_task_completion::create_verification_session;
use crate::store::AccountRecord;
use crate::ServerState;

pub(crate) type ApiError = (u16, &'static str, String);

pub(crate) struct MediaFlowContext<'a> {
    pub manifest: &'a PluginManifest,
    pub model: &'a ModelDef,
    pub account: AccountRecord,
    pub task_id: String,
}

enum FailureRoute<'a> {
    Verification(FailureDecision),
    RetrySameAccount,
    NextAccount(Box<MediaFlowContext<'a>>),
    Final(FailureDecision),
}

/// 统一解析图片、视频模型和可用账号，避免各媒体接口复制调度规则。
pub(crate) fn resolve_media_flow_context<'a>(
    state: &'a web::Data<ServerState>,
    model_id: &str,
    expected_kind: TaskKind,
    caller_key: Option<&str>,
    task_id: String,
) -> Result<MediaFlowContext<'a>, ApiError> {
    let (manifest, model) = state
        .plugin_loader
        .resolve_model(model_id)
        .ok_or_else(|| (404, "model_not_found", format!("模型 {model_id} 不存在")))?;
    if model.task_kind != expected_kind {
        return Err((
            400,
            "model_task_kind_mismatch",
            format!("模型 {model_id} 不支持当前媒体类型"),
        ));
    }
    let account = state
        .store
        .first_available_account_for_key(&manifest.platform_id, caller_key)
        .map(|account| state.store.account_with_required_runtime_proxy(account))
        .transpose()
        .map_err(|message| (409, "proxy_unavailable", message))?
        .ok_or_else(|| {
            (
                409,
                "account_unavailable",
                format!("{} 没有可用账号，请先添加账号。", manifest.display_name),
            )
        })?;
    Ok(MediaFlowContext {
        manifest,
        model,
        account,
        task_id,
    })
}

pub(crate) fn ensure_task_not_cancelled(
    state: &web::Data<ServerState>,
    task_id: &str,
) -> Result<(), ApiError> {
    if state
        .store
        .task_by_id(task_id)
        .is_some_and(|task| task.status == "cancelled")
    {
        Err((409, "task_cancelled", "用户取消".to_string()))
    } else {
        Ok(())
    }
}

pub(crate) fn classify_and_apply_failure(
    state: &web::Data<ServerState>,
    context: &MediaFlowContext<'_>,
    message: &str,
) -> FailureDecision {
    let decision = if let Some(message) = message.strip_prefix("needs_verification:") {
        FailureDecision {
            category: "verification_required".to_string(),
            retry_action: "failover".to_string(),
            account_state: Some("verification_required".to_string()),
            usage_consumed: false,
            same_error_threshold: 1,
            threshold_cooldown_seconds: 0,
            cooldown_from_message: false,
            cooldown_seconds: 0,
            original_message: message.to_string(),
        }
    } else {
        classify_failure(context.manifest, message)
    };
    apply_account_state_for_failure(state, context, &decision);
    if decision.account_state.is_some() {
        state.account_refresh.notify_schedule_changed();
    }
    if decision.account_state.is_some() {
        state.store.record_account_failure(
            &context.account.id,
            &decision.category,
            if context.model.task_kind == mediagen_core::TaskKind::Text {
                "text"
            } else {
                "media"
            },
        );
    }
    decision
}

pub(crate) fn record_attempt_failure(
    state: &web::Data<ServerState>,
    context: &MediaFlowContext<'_>,
    decision: &FailureDecision,
    task_label: &str,
) {
    if decision.usage_consumed {
        state.store.mark_account_usage(&context.account.id);
    }
    let message = decision.formatted_message();
    state
        .store
        .add_task_event(&context.task_id, "error", message.clone());
    state.store.add_audit_log(
        "engine",
        "error",
        format!("{task_label}任务失败"),
        message,
        Some(context.task_id.clone()),
    );
}

pub(crate) fn next_failover_context<'a>(
    state: &'a web::Data<ServerState>,
    current: &MediaFlowContext<'a>,
    caller_key: Option<&str>,
    excluded: &[String],
) -> Result<Option<MediaFlowContext<'a>>, ApiError> {
    let selected = state.store.with_account_dispatch(|| {
        let account = state.store.first_available_account_for_key_excluding(
            &current.manifest.platform_id,
            caller_key,
            excluded,
        )?;
        let account = state.store.account_with_required_runtime_proxy(account);
        if let Ok(account) = &account {
            state.store.set_task_account(&current.task_id, &account.id);
        }
        Some(account)
    });
    let Some(account) = selected else {
        return Ok(None);
    };
    let account = account.map_err(|message| (409, "proxy_unavailable", message))?;
    state.store.add_task_event(
        &current.task_id,
        "info",
        format!("切换账号继续执行: {}", account.id),
    );
    Ok(Some(MediaFlowContext {
        manifest: current.manifest,
        model: current.model,
        account,
        task_id: current.task_id.clone(),
    }))
}

/// 统一图片和视频的失败路由，保证重试、冷却和账号切换遵循同一套策略。
fn route_attempt_failure<'a>(
    state: &'a web::Data<ServerState>,
    context: &MediaFlowContext<'a>,
    message: &str,
    task_label: &str,
    same_account_failures: u32,
    caller_key: Option<&str>,
    excluded: &mut Vec<String>,
) -> Result<FailureRoute<'a>, ApiError> {
    let decision = classify_and_apply_failure(state, context, message);
    if decision.verification_required() {
        return Ok(FailureRoute::Verification(decision));
    }
    record_attempt_failure(state, context, &decision, task_label);
    if decision.should_retry_same_account(same_account_failures) {
        return Ok(FailureRoute::RetrySameAccount);
    }
    if decision.should_cooldown_after_retry_exhausted() {
        state.store.mark_account_state_for_seconds(
            &context.account.id,
            "cooldown",
            decision.threshold_cooldown_seconds,
        );
    }
    if !decision.should_failover() {
        return Ok(FailureRoute::Final(decision));
    }
    excluded.push(context.account.id.clone());
    match next_failover_context(state, context, caller_key, excluded)? {
        Some(next) => Ok(FailureRoute::NextAccount(Box::new(next))),
        None => Ok(FailureRoute::Final(decision)),
    }
}

/// 把失败策略转换为生成循环可直接执行的动作，避免媒体接口各自复制分支。
pub(crate) async fn handle_attempt_failure<'a>(
    state: &'a web::Data<ServerState>,
    context: &mut MediaFlowContext<'a>,
    message: &str,
    task_label: &str,
    same_account_failures: &mut u32,
    caller_key: Option<&str>,
    excluded: &mut Vec<String>,
) -> Result<Option<Result<Value, ApiError>>, ApiError> {
    *same_account_failures += 1;
    let route = route_attempt_failure(
        state,
        context,
        message,
        task_label,
        *same_account_failures,
        caller_key,
        excluded,
    )?;
    Ok(match route {
        FailureRoute::Verification(decision) => Some(
            create_verification_session(
                state,
                context.manifest,
                &context.account,
                &context.task_id,
                decision.original_message,
            )
            .await,
        ),
        FailureRoute::RetrySameAccount => None,
        FailureRoute::NextAccount(next) => {
            *context = *next;
            *same_account_failures = 0;
            None
        }
        FailureRoute::Final(decision) => Some(fail_task(state, context, &decision)),
    })
}

pub(crate) fn fail_task(
    state: &web::Data<ServerState>,
    context: &MediaFlowContext<'_>,
    decision: &FailureDecision,
) -> Result<serde_json::Value, ApiError> {
    ensure_task_not_cancelled(state, &context.task_id)?;
    let message = decision.formatted_message();
    state.store.fail_task(&context.task_id, message.clone());
    let (status, code) = decision.response_error();
    if code == "insufficient_quota" {
        return Err((
            status,
            code,
            "当前模型的上游账号额度为 0，请检查 API 额度或订阅".into(),
        ));
    }
    if status == 403 {
        return Err((
            status,
            code,
            "当前上游账号未获此能力授权，请检查订阅或账号权限".into(),
        ));
    }
    Err((status, code, message))
}

fn apply_account_state_for_failure(
    state: &web::Data<ServerState>,
    context: &MediaFlowContext<'_>,
    decision: &FailureDecision,
) {
    let Some(account_state) = decision.account_state.as_deref() else {
        return;
    };
    if account_state == "cooldown" {
        let fallback = i64::from(state.store.settings().cooldown_default_seconds);
        state.store.mark_account_state_for_seconds(
            &context.account.id,
            account_state,
            decision.cooldown_seconds(&decision.original_message, fallback),
        );
    } else if let Some(seconds) = decision
        .cooldown_seconds_from_message(&decision.original_message)
        .or_else(|| (decision.cooldown_seconds > 0).then_some(decision.cooldown_seconds))
    {
        state
            .store
            .mark_account_state_for_seconds(&context.account.id, account_state, seconds);
    } else {
        state
            .store
            .mark_account_state(&context.account.id, account_state);
    }
}
