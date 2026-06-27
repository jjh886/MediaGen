use std::collections::HashSet;

use actix_web::web;

use super::browser_lifecycle::{force_close_browser, force_close_browser_session};
use super::image::{
    cleanup_reference_files, find_poll_data, insert_processing_task, mark_task_completed,
    mark_task_failed, release_account_and_update_usage, ImageApiResponse,
};
use super::image_account::{
    allocate_account_when_available_excluding, ActiveAccountLease, PrepareError,
};
use super::image_output::process_poll_images;
use super::image_partials::collect_partial_images;
use super::ImageGenContext;
use crate::account_manager::handle_task_failure;
use crate::failure_strategy::{classify, default_failure_window_seconds, FailureDecision};
use crate::orchestrator::{FlowRunError, Orchestrator};
use crate::sync_ext::MutexExt;
use crate::types::{FailureRetryAction, LoginState, StepResult, TaskAttemptRecord};
use crate::AppState;

const SAME_ACCOUNT_RETRY_LIMIT: usize = 1;
const ACCOUNT_FAILOVER_LIMIT: usize = 2;

struct AttemptState {
    ctx: ImageGenContext,
    lease: Option<ActiveAccountLease>,
}

pub(crate) async fn generate_image_response_with_attempts(
    state: web::Data<AppState>,
    ctx: ImageGenContext,
    account_lease: ActiveAccountLease,
) -> Result<ImageApiResponse, PrepareError> {
    insert_processing_task(&state, &ctx);
    let mut current = AttemptState {
        ctx,
        lease: Some(account_lease),
    };
    let mut excluded = HashSet::new();
    let mut attempt_index = 1;
    let mut failovers = 0;
    let mut same_account_retries = 0;
    let mut action = "initial".to_string();
    let same_retry_limit = read_usize_setting(
        &state,
        "image_same_account_retry_limit",
        SAME_ACCOUNT_RETRY_LIMIT,
    );
    let failover_limit = read_usize_setting(
        &state,
        "image_account_failover_limit",
        ACCOUNT_FAILOVER_LIMIT,
    );

    loop {
        let attempt_id = start_attempt(&state, &current.ctx, attempt_index, &action);
        let result = Orchestrator::execute_flow_with_results(
            &current.ctx.flow,
            &state.engine_registry,
            current.ctx.initial_context.clone(),
        )
        .await;

        match result {
            Ok(results) => {
                let outcome =
                    finish_successful_flow(&state, &current.ctx, &attempt_id, results).await;
                match outcome {
                    Ok((response, step_results)) => {
                        release_current_account(&state, &mut current, Some(&step_results), true);
                        close_finished_browser(&state, &current.ctx).await;
                        cleanup_reference_files(&current.ctx);
                        return Ok(response);
                    }
                    Err(e) => {
                        release_current_account(&state, &mut current, None, false);
                        close_finished_browser(&state, &current.ctx).await;
                        cleanup_reference_files(&current.ctx);
                        return Err(e);
                    }
                }
            }
            Err(err) => {
                let decision = classify(
                    state.plugin_loader.get_plugin(&current.ctx.platform),
                    &err.error,
                    &err.results,
                );
                let usage_consumed = decision.usage_consumed.unwrap_or(false);
                finish_failed_attempt(&state, &attempt_id, &decision, usage_consumed);
                let threshold_blocked =
                    apply_failure_effects(&state, &current.ctx, &err, &decision);
                maybe_spawn_verification(&state, &current.ctx, &decision.message);

                if should_retry_same_account(
                    &decision,
                    threshold_blocked,
                    same_account_retries,
                    same_retry_limit,
                ) {
                    same_account_retries += 1;
                    attempt_index += 1;
                    action = "same_account_retry".to_string();
                    close_failed_browser(&state, &current.ctx).await;
                    continue;
                }

                if should_failover(&decision, threshold_blocked, failovers, failover_limit) {
                    failovers += 1;
                    attempt_index += 1;
                    action = "failover".to_string();
                    excluded.insert(current.ctx.account_id.clone());
                    close_failed_browser(&state, &current.ctx).await;
                    release_current_account(
                        &state,
                        &mut current,
                        Some(&err.results),
                        usage_consumed,
                    );
                    match allocate_failover_account(&state, &current.ctx, &excluded, attempt_index)
                        .await
                    {
                        Ok(next) => {
                            current = next;
                            same_account_retries = 0;
                            continue;
                        }
                        Err(e) => {
                            cleanup_reference_files(&current.ctx);
                            mark_task_failed(&state, &current.ctx, &decision.message);
                            return Err(e);
                        }
                    }
                }

                close_failed_browser(&state, &current.ctx).await;
                release_current_account(&state, &mut current, Some(&err.results), usage_consumed);
                cleanup_reference_files(&current.ctx);
                mark_task_failed(&state, &current.ctx, &decision.message);
                return Err((500, "generation_failed", decision.message));
            }
        }
    }
}

async fn finish_successful_flow(
    state: &web::Data<AppState>,
    ctx: &ImageGenContext,
    attempt_id: &str,
    results: Vec<StepResult>,
) -> Result<(ImageApiResponse, Vec<StepResult>), PrepareError> {
    let poll_data = find_poll_data(&results);
    let image_results = process_poll_images(poll_data, ctx, &results).await;
    if image_results.is_empty() {
        let message = "生成完成但没有拿到图片结果";
        finish_attempt(
            state,
            attempt_id,
            "failed",
            None,
            Some(message),
            Some(false),
        );
        mark_task_failed(state, ctx, message);
        return Err((500, "generation_failed", message.to_string()));
    }

    let first = image_results.first();
    let partial_images = collect_partial_images(poll_data, &ctx.output_format);
    let task_json = serde_json::json!({
        "width": first.map(|r| r.width).unwrap_or(0),
        "height": first.map(|r| r.height).unwrap_or(0),
        "imageCount": image_results.len(),
        "partialImageCount": partial_images.len(),
        "source": poll_data.and_then(|d| d["source"].as_str()).unwrap_or("unknown"),
    });
    mark_task_completed(state, ctx, &task_json);
    finish_attempt(state, attempt_id, "success", None, None, Some(true));
    clear_account_failures(state, &ctx.account_id);
    let images = image_results
        .into_iter()
        .map(|item| item.b64_json)
        .collect();
    let response = ImageApiResponse {
        created: chrono::Utc::now().timestamp(),
        images,
        partial_images,
        output_format: ctx.output_format.clone(),
        quality: ctx.quality.clone(),
        size: ctx.size.clone(),
        background: ctx.background.clone(),
    };
    Ok((response, results))
}

fn release_current_account(
    state: &web::Data<AppState>,
    current: &mut AttemptState,
    results: Option<&[StepResult]>,
    usage_consumed: bool,
) {
    if let Some(lease) = current.lease.take() {
        release_account_and_update_usage(state, &current.ctx, lease, results, usage_consumed);
    }
}

fn start_attempt(
    state: &web::Data<AppState>,
    ctx: &ImageGenContext,
    attempt_index: i32,
    action: &str,
) -> String {
    let id = format!("{}-{attempt_index}", ctx.request_id);
    let storage = state.storage.lock_recover();
    let _ = storage.update_task_account(&ctx.request_id, Some(&ctx.account_id));
    let attempt = TaskAttemptRecord {
        id: id.clone(),
        task_id: ctx.request_id.clone(),
        attempt_index,
        account_id: Some(ctx.account_id.clone()),
        action: action.to_string(),
        status: "running".to_string(),
        error_code: None,
        error_message: None,
        usage_consumed: None,
        started_at: chrono::Utc::now(),
        completed_at: None,
    };
    if let Err(e) = storage.insert_task_attempt(&attempt) {
        tracing::warn!(error = %e, task_id = %ctx.request_id, "保存任务尝试记录失败");
    }
    id
}

fn finish_failed_attempt(
    state: &web::Data<AppState>,
    attempt_id: &str,
    decision: &FailureDecision,
    usage_consumed: bool,
) {
    finish_attempt(
        state,
        attempt_id,
        "failed",
        decision.error_code.as_deref(),
        Some(&decision.message),
        Some(usage_consumed),
    );
}

fn finish_attempt(
    state: &web::Data<AppState>,
    attempt_id: &str,
    status: &str,
    error_code: Option<&str>,
    error_message: Option<&str>,
    usage_consumed: Option<bool>,
) {
    let storage = state.storage.lock_recover();
    if let Err(e) = storage.finish_task_attempt(
        attempt_id,
        status,
        error_code,
        error_message,
        usage_consumed,
    ) {
        tracing::warn!(error = %e, attempt_id, "更新任务尝试记录失败");
    }
}

fn apply_failure_effects(
    state: &web::Data<AppState>,
    ctx: &ImageGenContext,
    err: &FlowRunError,
    decision: &FailureDecision,
) -> bool {
    let storage = state.storage.lock_recover();
    handle_task_failure(
        &storage,
        &state.plugin_loader,
        &ctx.account_id,
        &err.error,
        &err.results,
    );
    maybe_apply_threshold_cooldown(&storage, ctx, decision)
}

fn maybe_apply_threshold_cooldown(
    storage: &crate::storage::Storage,
    ctx: &ImageGenContext,
    decision: &FailureDecision,
) -> bool {
    if decision.account_state.is_some()
        || decision.retry_action != FailureRetryAction::SameAccountRetry
    {
        return false;
    }
    let count = storage
        .record_account_failure(
            &ctx.account_id,
            &decision.fingerprint,
            default_failure_window_seconds(),
        )
        .unwrap_or(0);
    if count < decision.same_error_threshold {
        return false;
    }
    let cooldown_until =
        chrono::Utc::now() + chrono::Duration::seconds(decision.threshold_cooldown_seconds);
    let reason = format!(
        "连续 {} 次同类错误，临时暂停账号: {}",
        count,
        truncate(&decision.message, 120)
    );
    let _ = storage.update_login_state_with_cooldown(
        &ctx.account_id,
        LoginState::Cooldown,
        &reason,
        Some(cooldown_until),
    );
    true
}

fn should_retry_same_account(
    decision: &FailureDecision,
    threshold_blocked: bool,
    same_account_retries: usize,
    same_retry_limit: usize,
) -> bool {
    !threshold_blocked
        && decision.retry_action == FailureRetryAction::SameAccountRetry
        && same_account_retries < same_retry_limit
}

fn should_failover(
    decision: &FailureDecision,
    threshold_blocked: bool,
    failovers: usize,
    failover_limit: usize,
) -> bool {
    if failovers >= failover_limit {
        return false;
    }
    threshold_blocked
        || matches!(
            decision.retry_action,
            FailureRetryAction::Failover | FailureRetryAction::SameAccountRetry
        )
}

async fn allocate_failover_account(
    state: &web::Data<AppState>,
    previous_ctx: &ImageGenContext,
    excluded: &HashSet<String>,
    attempt_index: i32,
) -> Result<AttemptState, PrepareError> {
    let (account_id, profile_dir, account_cookies, lease) =
        allocate_account_when_available_excluding(
            state,
            &previous_ctx.platform,
            &previous_ctx.request_id,
            None,
            excluded,
        )
        .await?;
    let ctx = rebuild_context_for_account(
        previous_ctx,
        account_id,
        profile_dir,
        account_cookies,
        attempt_index,
    );
    Ok(AttemptState {
        ctx,
        lease: Some(lease),
    })
}

fn rebuild_context_for_account(
    ctx: &ImageGenContext,
    account_id: String,
    profile_dir: String,
    account_cookies: String,
    attempt_index: i32,
) -> ImageGenContext {
    let mut next = ctx.clone();
    next.account_id = account_id;
    next.profile_dir = profile_dir;
    next.account_cookies = account_cookies;
    next.browser_session_id = format!("img-{}-{attempt_index}", next.request_id);
    if let Some(map) = next.initial_context.as_object_mut() {
        map.insert("account_id".to_string(), serde_json::json!(next.account_id));
        map.insert(
            "profile_dir".to_string(),
            serde_json::json!(next.profile_dir),
        );
        map.insert(
            "account_cookies".to_string(),
            serde_json::json!(next.account_cookies),
        );
        map.insert(
            "browser_session_id".to_string(),
            serde_json::json!(next.browser_session_id),
        );
    }
    next
}

async fn close_failed_browser(state: &web::Data<AppState>, ctx: &ImageGenContext) {
    force_close_browser(
        &state.engine_registry,
        &state.plugin_loader,
        &ctx.platform,
        &ctx.profile_dir,
    )
    .await;
    force_close_browser_session(&state.engine_registry, &ctx.browser_session_id).await;
}

async fn close_finished_browser(state: &web::Data<AppState>, ctx: &ImageGenContext) {
    force_close_browser(
        &state.engine_registry,
        &state.plugin_loader,
        &ctx.platform,
        &ctx.profile_dir,
    )
    .await;
}

fn maybe_spawn_verification(state: &web::Data<AppState>, ctx: &ImageGenContext, message: &str) {
    if crate::account_verification::error_suggests_verification(
        state.plugin_loader.get_plugin(&ctx.platform),
        message,
    ) && !crate::account_verification::is_verification_task(ctx)
    {
        crate::account_verification::spawn(state.clone().into_inner(), ctx.clone());
    }
}

fn clear_account_failures(state: &web::Data<AppState>, account_id: &str) {
    let storage = state.storage.lock_recover();
    let _ = storage.clear_account_failures(account_id);
}

fn read_usize_setting(state: &web::Data<AppState>, key: &str, default_value: usize) -> usize {
    let storage = state.storage.lock_recover();
    storage
        .get_setting(key)
        .ok()
        .flatten()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default_value)
}

fn truncate(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}
