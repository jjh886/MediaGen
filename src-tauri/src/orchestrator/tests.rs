use super::*;
use crate::engine::AtomExecutor;
use async_trait::async_trait;
use serde_json::json;

struct MockEngine;

#[async_trait]
impl AtomExecutor for MockEngine {
    async fn execute(
        &self,
        _action: &str,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        Ok(StepResult {
            success: true,
            data: params.clone(),
            error: None,
        })
    }

    fn handles(&self, _prefix: &str) -> bool {
        true
    }
}

struct FailEngine;

#[async_trait]
impl AtomExecutor for FailEngine {
    async fn execute(
        &self,
        _action: &str,
        _params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        Ok(StepResult {
            success: false,
            data: json!({"error_code": "demo_failed"}),
            error: Some("模拟失败".to_string()),
        })
    }

    fn handles(&self, _prefix: &str) -> bool {
        true
    }
}

#[test]
fn test_resolve_templates() {
    let context = json!({
        "name": "Alice",
        "user": { "id": 42 }
    });
    let template = json!({
        "greeting": "Hello {{name}}",
        "uid": "user-{{user.id}}"
    });
    let resolved = resolve_templates(&template, &context);
    assert_eq!(resolved["greeting"], "Hello Alice");
    assert_eq!(resolved["uid"], "user-42");
}

#[tokio::test]
async fn test_execute_flow_basic() {
    let mut registry = EngineRegistry::new();
    registry.register(Box::new(MockEngine));
    let flow = FlowDef {
        steps: vec![
            crate::types::StepDef {
                action: "mock.step1".to_string(),
                params: json!({"key": "val1"}),
            },
            crate::types::StepDef {
                action: "mock.step2".to_string(),
                params: json!({"key": "val2"}),
            },
        ],
    };
    let results = Orchestrator::execute_flow(&flow, &registry, json!({}))
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].data["key"], "val1");
    assert_eq!(results[1].data["key"], "val2");
}

#[tokio::test]
async fn test_save_as() {
    let mut registry = EngineRegistry::new();
    registry.register(Box::new(MockEngine));
    let flow = FlowDef {
        steps: vec![
            crate::types::StepDef {
                action: "mock.first".to_string(),
                params: json!({"save_as": "first_result", "value": "hello"}),
            },
            crate::types::StepDef {
                action: "mock.second".to_string(),
                params: json!({"ref": "{{first_result.value}}"}),
            },
        ],
    };
    let results = Orchestrator::execute_flow(&flow, &registry, json!({}))
        .await
        .unwrap();
    assert_eq!(results[1].data["ref"], "hello");
}

#[tokio::test]
async fn test_step_failure_reports_results() {
    let mut registry = EngineRegistry::new();
    registry.register(Box::new(FailEngine));
    let flow = FlowDef {
        steps: vec![
            crate::types::StepDef {
                action: "mock.fail".to_string(),
                params: json!({}),
            },
            crate::types::StepDef {
                action: "mock.never_reached".to_string(),
                params: json!({}),
            },
        ],
    };
    let err = Orchestrator::execute_flow_with_results(&flow, &registry, json!({}))
        .await
        .unwrap_err();
    assert_eq!(err.results.len(), 1);
    assert_eq!(err.results[0].data["error_code"], "demo_failed");
}

#[tokio::test]
async fn test_skip_if_true_skips_step() {
    let mut registry = EngineRegistry::new();
    registry.register(Box::new(MockEngine));
    let flow = FlowDef {
        steps: vec![crate::types::StepDef {
            action: "mock.step".to_string(),
            params: json!({"save_as": "s1", "skip_if": true, "value": "x"}),
        }],
    };
    let results = Orchestrator::execute_flow(&flow, &registry, json!({}))
        .await
        .unwrap();
    assert_eq!(results[0].data["skipped"], true);
}

#[tokio::test]
async fn test_skip_if_empty_skips_missing_text() {
    let mut registry = EngineRegistry::new();
    registry.register(Box::new(MockEngine));
    let flow = FlowDef {
        steps: vec![crate::types::StepDef {
            action: "browser.click_text".to_string(),
            params: json!({"save_as": "s1", "text": "", "skip_if_empty": true}),
        }],
    };
    let results = Orchestrator::execute_flow(&flow, &registry, json!({}))
        .await
        .unwrap();
    assert_eq!(results[0].data["skipped"], true);
}
