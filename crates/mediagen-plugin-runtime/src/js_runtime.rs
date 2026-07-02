use async_trait::async_trait;
use rquickjs::{prelude::Func, CatchResultExt, CaughtError, Context, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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
}

pub async fn run_js_step<H>(script: String, context: Value, host: H) -> Result<Value, String>
where
    H: PluginHost,
{
    let context_json = context.to_string();
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || run_js_blocking(script, context_json, host, handle))
        .await
        .map_err(|err| format!("plugin.run_js 执行线程失败: {err}"))?
}

fn run_js_blocking<H>(
    script: String,
    context_json: String,
    host: H,
    handle: Handle,
) -> Result<Value, String>
where
    H: PluginHost,
{
    let runtime = Runtime::new().map_err(|err| format!("创建 JS runtime 失败: {err}"))?;
    let context = Context::full(&runtime).map_err(|err| format!("创建 JS context 失败: {err}"))?;

    context.with(|ctx| {
        ctx.globals()
            .set("__contextJson", context_json)
            .map_err(js_err)?;
        let host_for_call = host.clone();
        let handle_for_call = handle.clone();
        ctx.globals()
            .set(
                "hostCall",
                Func::from(move |action: String, params_json: String| -> String {
                    host_call(&host_for_call, &handle_for_call, action, params_json)
                }),
            )
            .map_err(js_err)?;
        ctx.globals()
            .set(
                "hostSleep",
                Func::from(|ms: u32| {
                    // 插件脚本可 sleep，但单次最多 5 分钟，避免错误脚本永久卡死线程。
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
        serde_json::from_str(&output).map_err(|err| format!("plugin.run_js 返回值不是 JSON: {err}"))
    })
}

fn host_call<H>(host: &H, handle: &Handle, action: String, params_json: String) -> String
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
        handle.block_on(host.call(&action, &params))
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
