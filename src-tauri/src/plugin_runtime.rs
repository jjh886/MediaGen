use rquickjs::{prelude::Func, CatchResultExt, CaughtError, Context, Runtime};
use serde_json::{json, Value};
use tokio::runtime::Handle;

use crate::engine::EngineRegistry;
use crate::error::AppError;
use crate::types::StepResult;

const PRELUDE: &str = r#"
globalThis.__context = JSON.parse(globalThis.__contextJson);
globalThis.host = {
  call(action, params) {
    const result = JSON.parse(hostCall(String(action), JSON.stringify(params || {})));
    if (!result.success) {
      throw new Error(result.error || `host action failed: ${action}`);
    }
    return result.data;
  },
  tryCall(action, params) {
    return JSON.parse(hostCall(String(action), JSON.stringify(params || {})));
  },
  sleep(ms) {
    hostSleep(Number(ms) || 0);
  }
};
"#;

pub async fn run_js_step(
    params: &Value,
    context: &Value,
    engine: EngineRegistry,
) -> Result<StepResult, AppError> {
    let script = params["script"]
        .as_str()
        .ok_or_else(|| AppError::PlatformError("plugin.run_js 缺少 script".into()))?
        .to_string();
    let context_json = context.to_string();
    let handle = Handle::current();

    let data =
        tokio::task::spawn_blocking(move || run_js_blocking(script, context_json, engine, handle))
            .await
            .map_err(|e| AppError::PlatformError(format!("plugin.run_js 执行线程失败: {e}")))??;

    Ok(StepResult {
        success: true,
        data,
        error: None,
    })
}

fn run_js_blocking(
    script: String,
    context_json: String,
    engine: EngineRegistry,
    handle: Handle,
) -> Result<Value, AppError> {
    let runtime = Runtime::new()
        .map_err(|e| AppError::PlatformError(format!("创建 JS runtime 失败: {e}")))?;
    let context = Context::full(&runtime)
        .map_err(|e| AppError::PlatformError(format!("创建 JS context 失败: {e}")))?;

    context.with(|ctx| {
        ctx.globals()
            .set("__contextJson", context_json)
            .map_err(js_err)?;
        let host_engine = engine.cloned();
        let host_handle = handle.clone();
        ctx.globals()
            .set(
                "hostCall",
                Func::from(move |action: String, params_json: String| -> String {
                    host_call(&host_engine, &host_handle, action, params_json)
                }),
            )
            .map_err(js_err)?;
        ctx.globals()
            .set(
                "hostSleep",
                Func::from(|ms: u32| {
                    let capped_ms = ms.min(300_000);
                    std::thread::sleep(std::time::Duration::from_millis(capped_ms as u64));
                }),
            )
            .map_err(js_err)?;
        ctx.eval::<(), _>(PRELUDE)
            .catch(&ctx)
            .map_err(js_caught_err)?;
        ctx.eval::<(), _>(script)
            .catch(&ctx)
            .map_err(js_caught_err)?;
        let output = ctx
            .eval::<String, _>(
                "JSON.stringify(typeof main === 'function' ? main(globalThis.__context) : null)",
            )
            .catch(&ctx)
            .map_err(js_caught_err)?;
        serde_json::from_str(&output)
            .map_err(|e| AppError::PlatformError(format!("plugin.run_js 返回值不是 JSON: {e}")))
    })
}

fn host_call(
    engine: &EngineRegistry,
    handle: &Handle,
    action: String,
    params_json: String,
) -> String {
    let params = serde_json::from_str::<Value>(&params_json).unwrap_or_else(|e| {
        json!({
            "__invalid_params": true,
            "error": e.to_string()
        })
    });
    let result = if params["__invalid_params"].as_bool().unwrap_or(false) {
        StepResult {
            success: false,
            data: Value::Null,
            error: Some(format!("host.call 参数不是 JSON: {}", params["error"])),
        }
    } else if action == "plugin.run_js" {
        StepResult {
            success: false,
            data: Value::Null,
            error: Some("plugin.run_js 不允许递归调用".to_string()),
        }
    } else {
        match handle.block_on(engine.execute(&action, &params)) {
            Ok(result) => result,
            Err(e) => StepResult {
                success: false,
                data: Value::Null,
                error: Some(e.to_string()),
            },
        }
    };
    serde_json::to_string(&result).unwrap_or_else(|e| {
        json!({
            "success": false,
            "data": null,
            "error": format!("host.call 结果序列化失败: {e}")
        })
        .to_string()
    })
}

fn js_err(error: rquickjs::Error) -> AppError {
    AppError::PlatformError(format!("plugin.run_js JS 错误: {error}"))
}

fn js_caught_err(error: CaughtError<'_>) -> AppError {
    AppError::PlatformError(format!("plugin.run_js JS 错误: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::AtomExecutor;
    use async_trait::async_trait;

    struct EchoEngine;

    #[async_trait]
    impl AtomExecutor for EchoEngine {
        async fn execute(&self, _action: &str, params: &Value) -> Result<StepResult, AppError> {
            Ok(StepResult {
                success: true,
                data: params.clone(),
                error: None,
            })
        }

        fn handles(&self, action_prefix: &str) -> bool {
            action_prefix == "echo"
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_js_can_call_host_actions() {
        let mut registry = EngineRegistry::new();
        registry.register(Box::new(EchoEngine));
        let result = run_js_step(
            &json!({
                "script": "function main(ctx) { const value = host.call('echo.value', { value: ctx.input }); return { value: value.value }; }"
            }),
            &json!({ "input": "ok" }),
            registry,
        )
        .await
        .unwrap();

        assert_eq!(result.data["value"], "ok");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_js_reports_thrown_error_message() {
        let registry = EngineRegistry::new();
        let error = run_js_step(
            &json!({
                "script": "function main() { throw new Error('token exchange failed: HTTP 403'); }"
            }),
            &json!({}),
            registry,
        )
        .await
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("token exchange failed: HTTP 403"));
        assert!(!message.contains("Exception generated by QuickJS"));
    }
}
