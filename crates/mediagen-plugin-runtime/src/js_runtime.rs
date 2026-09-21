use async_trait::async_trait;
use rquickjs::{prelude::Func, CatchResultExt, CaughtError, Context, Function, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;

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
  emit(event) {
    host.call("stream.emit", { event });
  },
  checkpoint(value) {
    return host.call("checkpoint", value === undefined ? {} : { value }) || {};
  },
  stream(action, params, onEvent) {
    const result = JSON.parse(hostStream(String(action), JSON.stringify(params || {}), (raw) => {
      const mapped = onEvent(JSON.parse(raw));
      if (mapped != null) host.emit(mapped);
    }));
    if (!result.success) throw new Error(result.error || "事件流失败");
    return result.data;
  },
  sleep(ms) {
    hostSleep(Number(ms) || 0);
  }
};
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginStepResult {
    pub success: bool,
    pub data: Value,
    pub error: Option<String>,
}

#[async_trait]
pub trait PluginHost: Clone + Send + Sync + 'static {
    async fn call(&self, action: &str, params: &Value) -> PluginStepResult;
    async fn stream(
        &self,
        action: &str,
        params: &Value,
        _events: tokio::sync::mpsc::Sender<Value>,
    ) -> PluginStepResult {
        self.call(action, params).await
    }
}

pub async fn run_js_step<H>(script: String, context: Value, host: H) -> Result<Value, String>
where
    H: PluginHost,
{
    let cancelled = Arc::new(AtomicBool::new(false));
    let _guard = CancelOnDrop(cancelled.clone());
    let context_json = context.to_string();
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || {
        run_js_blocking(script, context_json, host, handle, cancelled)
    })
    .await
    .map_err(|err| format!("plugin.run_js 执行线程失败: {err}"))?
}

fn run_js_blocking<H>(
    script: String,
    context_json: String,
    host: H,
    handle: Handle,
    cancelled: Arc<AtomicBool>,
) -> Result<Value, String>
where
    H: PluginHost,
{
    let runtime = Runtime::new().map_err(|err| format!("创建 JS runtime 失败: {err}"))?;
    runtime.set_memory_limit(256 * 1024 * 1024);
    let interrupted = cancelled.clone();
    let deadline = Instant::now() + Duration::from_secs(3600);
    runtime.set_interrupt_handler(Some(Box::new(move || {
        interrupted.load(Ordering::Relaxed) || Instant::now() > deadline
    })));
    let context = Context::full(&runtime).map_err(|err| format!("创建 JS context 失败: {err}"))?;

    context.with(|ctx| {
        ctx.globals()
            .set("__contextJson", context_json)
            .map_err(js_err)?;
        let host_for_call = host.clone();
        let call_cancelled = cancelled.clone();
        let handle_for_call = handle.clone();
        ctx.globals()
            .set(
                "hostCall",
                Func::from(move |action: String, params_json: String| -> String {
                    host_call(
                        &host_for_call,
                        &handle_for_call,
                        action,
                        params_json,
                        &call_cancelled,
                    )
                }),
            )
            .map_err(js_err)?;
        let stream_host = host.clone();
        let stream_cancelled = cancelled.clone();
        let stream_handle = handle.clone();
        ctx.globals()
            .set(
                "hostStream",
                Func::from(
                    move |action: String,
                          raw: String,
                          callback: Function<'_>|
                          -> rquickjs::Result<String> {
                        let params: Value =
                            serde_json::from_str(&raw).map_err(|_| rquickjs::Error::Unknown)?;
                        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
                        let host = stream_host.clone();
                        let job = stream_handle
                            .spawn(async move { host.stream(&action, &params, sender).await });
                        loop {
                            if stream_cancelled.load(Ordering::Relaxed) {
                                job.abort();
                                return Err(rquickjs::Error::Unknown);
                            }
                            let next = stream_handle.block_on(tokio::time::timeout(
                                Duration::from_millis(200),
                                receiver.recv(),
                            ));
                            let event = match next {
                                Ok(Some(event)) => event,
                                Ok(None) => break,
                                Err(_) => continue,
                            };

                            if let Err(error) = callback.call::<_, ()>((event.to_string(),)) {
                                job.abort();
                                return Err(error);
                            }
                        }
                        let result = stream_handle
                            .block_on(job)
                            .map_err(|_| rquickjs::Error::Unknown)?;
                        serde_json::to_string(&result).map_err(|_| rquickjs::Error::Unknown)
                    },
                ),
            )
            .map_err(js_err)?;
        let sleep_cancelled = cancelled.clone();
        ctx.globals()
            .set(
                "hostSleep",
                Func::from(move |ms: u32| {
                    // 插件脚本可 sleep，但单次最多 5 分钟，避免错误脚本永久卡死线程。
                    let capped_ms = ms.min(300_000);
                    let until = Instant::now() + Duration::from_millis(capped_ms as u64);
                    while Instant::now() < until && !sleep_cancelled.load(Ordering::Relaxed) {
                        std::thread::sleep(
                            Duration::from_millis(20)
                                .min(until.saturating_duration_since(Instant::now())),
                        );
                    }
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
        serde_json::from_str(&output).map_err(|err| format!("plugin.run_js 返回值不是 JSON: {err}"))
    })
}

fn host_call<H>(
    host: &H,
    handle: &Handle,
    action: String,
    params_json: String,
    cancelled: &Arc<AtomicBool>,
) -> String
where
    H: PluginHost,
{
    let params = serde_json::from_str::<Value>(&params_json).unwrap_or_else(|err| {
        json!({
            "__invalid_params": true,
            "error": err.to_string()
        })
    });
    let result = if params["__invalid_params"].as_bool().unwrap_or(false) {
        PluginStepResult {
            success: false,
            data: Value::Null,
            error: Some(format!("host.call 参数不是 JSON: {}", params["error"])),
        }
    } else if action == "plugin.run_js" {
        PluginStepResult {
            success: false,
            data: Value::Null,
            error: Some("plugin.run_js 不允许递归调用".to_string()),
        }
    } else {
        handle.block_on(async {
            let request = host.call(&action, &params);
            tokio::pin!(request);
            loop {
                tokio::select! {
                    result = &mut request => return result,
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        if cancelled.load(Ordering::Relaxed) {
                            return PluginStepResult { success:false, data:Value::Null, error:Some("请求已取消".into()) };
                        }
                    }
                }
            }
        })
    };
    serde_json::to_string(&result).unwrap_or_else(|err| {
        json!({
            "success": false,
            "data": null,
            "error": format!("host.call 结果序列化失败: {err}")
        })
        .to_string()
    })
}

fn js_err(error: rquickjs::Error) -> String {
    format!("plugin.run_js JS 错误: {error}")
}

fn js_caught_err(error: CaughtError<'_>) -> String {
    format!("plugin.run_js JS 错误: {error}")
}

struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
#[path = "js_runtime_tests.rs"]
mod tests;
