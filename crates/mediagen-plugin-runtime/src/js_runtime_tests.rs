use super::*;
#[derive(Clone, Default)]
struct Host(Arc<std::sync::Mutex<Value>>);
#[async_trait]
impl PluginHost for Host {
    async fn call(&self, action: &str, params: &Value) -> PluginStepResult {
        let data = if action == "checkpoint" {
            let mut stored = self.0.lock().unwrap();
            if let Some(value) = params.get("value") {
                *stored = value.clone();
            }
            stored.clone()
        } else {
            params.clone()
        };
        PluginStepResult {
            success: true,
            data,
            error: None,
        }
    }
    async fn stream(
        &self,
        _action: &str,
        _params: &Value,
        events: tokio::sync::mpsc::Sender<Value>,
    ) -> PluginStepResult {
        events.send(json!({"delta":"你好"})).await.unwrap();
        events.send(json!({"delta":"世界"})).await.unwrap();
        PluginStepResult {
            success: true,
            data: json!({"ok":true}),
            error: None,
        }
    }
}
#[tokio::test]
async fn stream_callbacks_preserve_events_and_checkpoints() {
    let script="function main(){let text='';host.stream('http.sse_request',{},event=>{text+=event.delta;return null});host.checkpoint({text});return host.checkpoint()}";
    let result = run_js_step(script.into(), json!({}), Host::default())
        .await
        .unwrap();
    assert_eq!(result["text"], "你好世界");
}
#[tokio::test]
async fn cancelled_cpu_loop_does_not_keep_worker_alive() {
    let running = run_js_step(
        "function main(){while(true){}}".into(),
        json!({}),
        Host::default(),
    );
    assert!(tokio::time::timeout(Duration::from_millis(20), running)
        .await
        .is_err());
}

#[tokio::test]
async fn bounded_promise_jobs_reenter_host_and_reject_infinite_microtasks() {
    let result = run_js_step("function main(){let value;Promise.resolve().then(()=>host.call('echo',{number:42})).then(result=>{value=result});host.drain_jobs();return value}".into(),json!({}),Host::default()).await.unwrap();
    assert_eq!(result["number"], 42);
    let error = run_js_step("function main(){function repeat(){Promise.resolve().then(repeat)}repeat();host.drain_jobs(20);return {}}".into(),json!({}),Host::default()).await.unwrap_err();
    assert!(error.contains("异步回调超过"));
}
