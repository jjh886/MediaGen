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
        params: &Value,
        events: tokio::sync::mpsc::Sender<Value>,
    ) -> PluginStepResult {
        if params["delay"] == true {
            tokio::time::sleep(Duration::from_millis(350)).await;
        }
        if params["track"] == true {
            *self.0.lock().unwrap() = json!("finished");
        }
        events.send(json!({"delta":"你好"})).await.unwrap();
        events.send(json!({"delta":"世界"})).await.unwrap();
        PluginStepResult {
            success: true,
            data: json!({"ok":true,"params":params}),
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

#[tokio::test]
async fn idle_callbacks_can_call_host_without_polluting_transport_parameters() {
    let script = "function main(){let idle=0,text='';const result=host.stream('http.sse_request',{delay:true,idle_interval_ms:100},event=>{if(event.type==='http_idle'){idle++;host.call('checkpoint',{value:idle})}else{text+=event.delta}});return {idle,text,result,checkpoint:host.checkpoint()}}";
    let result = run_js_step(script.into(), json!({}), Host::default())
        .await
        .unwrap();
    assert!(result["idle"].as_u64().unwrap() >= 1);
    assert_eq!(result["text"], "你好世界");
    assert!(result["result"]["params"].get("idle_interval_ms").is_none());
    assert_eq!(result["checkpoint"], result["idle"]);
}

#[tokio::test]
async fn idle_callbacks_are_opt_in_and_intervals_are_validated() {
    let script = "function main(){let events=[];host.stream('http.sse_request',{delay:true},event=>events.push(event));return events}";
    let result = run_js_step(script.into(), json!({}), Host::default())
        .await
        .unwrap();
    assert_eq!(result.as_array().unwrap().len(), 2);
    for interval in [
        json!(0),
        json!(99),
        json!(60001),
        json!(-1),
        json!(100.5),
        json!("100"),
        Value::Null,
    ] {
        let script = format!("function main(){{host.stream('http.sse_request',{{idle_interval_ms:{interval}}},()=>{{}})}}");
        let error = run_js_step(script, json!({}), Host::default())
            .await
            .unwrap_err();
        assert!(error.contains("idle_interval_ms"));
    }
    let error = run_js_step("function main(){host.stream('http.sse_request',{delay:true,idle_interval_ms:100},()=>{throw new Error('保活失败')})}".into(), json!({}), Host::default()).await.unwrap_err();
    assert!(error.contains("保活失败"));
}

#[tokio::test]
async fn failed_idle_callback_and_cancelled_script_abort_the_transport() {
    for fail_callback in [true, false] {
        let host = Host::default();
        let script = format!("function main(){{host.stream('http.sse_request',{{delay:true,track:true,idle_interval_ms:100}},()=>{{if({fail_callback})throw new Error('保活失败')}})}}");
        let running = run_js_step(script, json!({}), host.clone());
        if fail_callback {
            assert!(running.await.unwrap_err().contains("保活失败"));
        } else {
            assert!(tokio::time::timeout(Duration::from_millis(30), running)
                .await
                .is_err());
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(*host.0.lock().unwrap(), Value::Null);
    }
}
