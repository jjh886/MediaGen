use crate::{store::AccountRecord, ServerState};
use actix_web::{web, HttpRequest, HttpResponse};
use mediagen_core::{ModelDef, PluginManifest};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
pub(crate) struct Catalog(
    pub Arc<Mutex<HashMap<String, CachedModels>>>,
    Arc<tokio::sync::Mutex<()>>,
);
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct CachedModels {
    models: Vec<ModelDef>,
    refreshed_at: i64,
}
impl Catalog {
    pub(crate) fn load(path: &std::path::Path) -> Self {
        let cache = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self(Arc::new(Mutex::new(cache)), Default::default())
    }
}

#[derive(Deserialize)]
pub(crate) struct Query {
    #[serde(default)]
    refresh: bool,
    #[serde(default)]
    account_id: Option<String>,
}

pub(crate) fn resolve(
    state: &ServerState,
    id: &str,
    key: &str,
) -> Option<(PluginManifest, ModelDef, Option<String>)> {
    let declared = state.plugin_loader.resolve_model(id);
    let canonical = declared
        .map(|(_, model)| model.model_id.as_str())
        .unwrap_or(id);
    let accounts = visible_accounts(state, key);
    let cache = state
        .model_catalog
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    for account in &accounts {
        if declared.is_some_and(|(manifest, _)| manifest.platform_id != account.platform) {
            continue;
        }
        let Some(model) = cache.get(&account.id).and_then(|entry| {
            entry
                .models
                .iter()
                .find(|model| model.model_id == id || model.model_id == canonical)
        }) else {
            continue;
        };
        let manifest = state.plugin_loader.get_plugin(&account.platform)?;
        let mut resolved = model.clone();
        let variants = accounts
            .iter()
            .filter(|other| other.platform == account.platform)
            .filter_map(|other| cache.get(&other.id))
            .filter_map(|entry| {
                entry
                    .models
                    .iter()
                    .find(|candidate| candidate.model_id == model.model_id)
            })
            .filter_map(|candidate| candidate.extra.get("capabilities"))
            .cloned()
            .collect::<Vec<_>>();
        resolved
            .extra
            .insert("capabilities".into(), merge_capabilities(&variants));
        return Some((manifest.clone(), resolved, None));
    }
    let (manifest, model) = declared?;
    if model.task_kind == mediagen_core::TaskKind::Text
        && accounts.iter().any(|account| {
            account.platform == manifest.platform_id && cache.contains_key(&account.id)
        })
    {
        return None;
    }
    Some((manifest.clone(), model.clone(), None))
}

pub(crate) fn account_model(
    state: &ServerState,
    manifest: &PluginManifest,
    model_id: &str,
    account_id: &str,
) -> Option<ModelDef> {
    let declared = state
        .plugin_loader
        .resolve_model(model_id)
        .map(|(_, model)| model);
    let canonical = declared
        .map(|model| model.model_id.as_str())
        .unwrap_or(model_id);
    let cache = state
        .model_catalog
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(model) = cache.get(account_id).and_then(|entry| {
        entry
            .models
            .iter()
            .find(|model| model.model_id == canonical || model.model_id == model_id)
    }) {
        return Some(model.clone());
    }
    let declared = declared?;
    if declared.task_kind == mediagen_core::TaskKind::Text && cache.contains_key(account_id) {
        return None;
    }
    manifest
        .models
        .iter()
        .find(|model| model.model_id == canonical)
        .cloned()
}

fn visible_accounts(state: &ServerState, key: &str) -> Vec<AccountRecord> {
    state
        .store
        .list_accounts(None)
        .into_iter()
        .filter(|account| {
            state.store.global_api_key_allowed(key)
                || crate::access_control::account_key_allowed(&account.api_key, key)
        })
        .collect()
}

pub(crate) async fn list(
    state: web::Data<ServerState>,
    request: HttpRequest,
    query: web::Query<Query>,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return crate::protocol_api::error(401, "invalid_api_key", "API Key 无效");
    }
    let key = crate::routes::api_key_from_request(&request).unwrap_or_default();
    let accounts = visible_accounts(&state, &key)
        .into_iter()
        .filter(|account| query.account_id.as_ref().is_none_or(|id| id == &account.id))
        .collect::<Vec<_>>();
    if query.account_id.is_some() && accounts.is_empty() {
        return crate::protocol_api::error(404, "account_not_found", "账号不存在或无权访问");
    }
    let mut failures = Vec::new();
    for account in accounts {
        if account.login_state == "disabled" || account.login_state == "login_expired" {
            continue;
        }
        let stale = state
            .model_catalog
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&account.id)
            .is_some_and(|entry| chrono::Utc::now().timestamp() - entry.refreshed_at > 3600);
        if !query.refresh && !stale {
            continue;
        }
        if let Err(code) = refresh_account(&state, account.clone()).await {
            failures.push(json!({"account_id":account.id,"code":code}));
        }
    }
    let data = catalog(&state, &key);
    HttpResponse::Ok().json(json!({"object":"list","data":data,"refresh_errors":failures}))
}

pub(crate) async fn refresh_account(
    state: &ServerState,
    account: AccountRecord,
) -> Result<(), &'static str> {
    let _refresh = state.model_catalog.1.lock().await;
    let manifest = state
        .plugin_loader
        .get_plugin(&account.platform)
        .ok_or("plugin_not_found")?;
    if !manifest.flows.contains_key("list_models") {
        return Ok(());
    }
    let account = state
        .store
        .account_with_required_runtime_proxy(account)
        .map_err(|_| "proxy_unavailable")?;
    let result = crate::protocol_api::run_plugin(
        state,
        manifest,
        &account,
        "list_models",
        json!({}),
        None,
        "",
        Value::Null,
    )
    .await
    .map_err(|_| "model_discovery_failed")?;
    let models: Vec<ModelDef> =
        serde_json::from_value(result["models"].clone()).map_err(|_| "invalid_model_catalog")?;
    if models.len() > 2048
        || models
            .iter()
            .any(|model| model.model_id.trim().is_empty() || model.model_id.len() > 256)
    {
        return Err("invalid_model_catalog");
    }
    let raw = {
        let mut cache = state
            .model_catalog
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        cache.insert(
            account.id,
            CachedModels {
                models,
                refreshed_at: chrono::Utc::now().timestamp(),
            },
        );
        serde_json::to_vec(&*cache).map_err(|_| "catalog_storage_failed")?
    };
    let path = state.store.path.with_file_name("model-catalog.json");
    crate::account_login_credentials::write_private_file(&path, &raw)
        .await
        .map_err(|_| "catalog_storage_failed")
}

pub(crate) async fn retrieve(
    state: web::Data<ServerState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return crate::protocol_api::error(401, "invalid_api_key", "API Key 无效");
    }
    let key = crate::routes::api_key_from_request(&request).unwrap_or_default();
    match catalog(&state, &key)
        .into_iter()
        .find(|item| item["id"] == path.as_str())
    {
        Some(model) => HttpResponse::Ok().json(model),
        None => crate::protocol_api::error(404, "model_not_found", "模型不存在"),
    }
}

fn catalog(state: &ServerState, key: &str) -> Vec<Value> {
    let accounts = visible_accounts(state, key);
    let cache = state
        .model_catalog
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let refreshed = |platform: &str| {
        accounts
            .iter()
            .any(|account| account.platform == platform && cache.contains_key(&account.id))
    };
    let mut entries = BTreeMap::new();
    for (manifest, model) in state.plugin_loader.all_models() {
        if model.task_kind == mediagen_core::TaskKind::Text && refreshed(&manifest.platform_id) {
            continue;
        }
        entries
            .entry(model.model_id.clone())
            .or_insert_with(|| model_value(manifest, model, &model.model_id));
    }
    for account in &accounts {
        let Some(manifest) = state.plugin_loader.get_plugin(&account.platform) else {
            continue;
        };
        for model in cache
            .get(&account.id)
            .into_iter()
            .flat_map(|entry| &entry.models)
        {
            entries
                .entry(model.model_id.clone())
                .or_insert_with(|| model_value(manifest, model, &model.model_id));
        }
    }
    for (alias, manifest, model) in state.plugin_loader.model_aliases() {
        if model.task_kind == mediagen_core::TaskKind::Text
            && !entries.contains_key(&model.model_id)
        {
            continue;
        }
        entries
            .entry(alias.clone())
            .or_insert_with(|| model_value(manifest, model, &alias));
    }
    for value in entries.values_mut() {
        let canonical = value["alias_of"]
            .as_str()
            .unwrap_or_else(|| value["id"].as_str().unwrap_or_default())
            .to_string();
        let platform = value["owned_by"].as_str().unwrap_or_default().to_string();
        let variants = accounts.iter().filter(|account| account.platform == platform).filter_map(|account| {
            let entry = cache.get(&account.id)?;
            let model = entry.models.iter().find(|model| model.model_id == canonical)?;
            let manifest = state.plugin_loader.get_plugin(&platform)?;
            Some(json!({"account_id":account.id,"capabilities":model_capabilities(manifest, model),"refreshed_at":entry.refreshed_at}))
        }).collect::<Vec<_>>();
        if !variants.is_empty() {
            value["capabilities"] = merge_capabilities(
                &variants
                    .iter()
                    .map(|variant| variant["capabilities"].clone())
                    .collect::<Vec<_>>(),
            );
            value["account_capabilities"] = json!(variants);
        }
    }
    entries.into_values().collect()
}

fn model_value(manifest: &PluginManifest, model: &ModelDef, id: &str) -> Value {
    let mut result = json!({"id":id,"object":"model","created":0,"owned_by":manifest.platform_id,"display_name":model.display_name,"task_kind":model.task_kind,"capabilities":model_capabilities(manifest,model)});
    if id != model.model_id {
        result["alias_of"] = json!(model.model_id);
    }
    result
}

fn model_capabilities(manifest: &PluginManifest, model: &ModelDef) -> Value {
    if let Some(value) = model.extra.get("capabilities").filter(|_| {
        model.task_kind != mediagen_core::TaskKind::Image
            && model.task_kind != mediagen_core::TaskKind::Video
    }) {
        return value.clone();
    }
    let (kind, flow, endpoint) = match model.task_kind {
        mediagen_core::TaskKind::Image => ("image", "generate_image", "/v1/images/generations"),
        mediagen_core::TaskKind::Video => ("video", "generate_video", "/v1/videos"),
        _ => return json!({}),
    };
    let limits = manifest
        .extra
        .get(&format!("{kind}_capabilities"))
        .cloned()
        .unwrap_or(json!({}));
    let limits = crate::image_parameters::model_limits(
        &limits,
        model.extra.get("capabilities").unwrap_or(&Value::Null),
    );
    let mut endpoints = Vec::new();
    if manifest.flows.contains_key(flow) {
        endpoints.push(endpoint);
    }
    if kind == "image" && limits["supports_edit"] == true {
        endpoints.push("/v1/images/edits");
    }
    if kind == "image" && limits["supports_variation"] == true {
        endpoints.push("/v1/images/variations");
    }
    let modalities = if limits["max_reference_images"]
        .as_u64()
        .is_some_and(|count| count > 0)
    {
        vec!["text", "image"]
    } else {
        vec!["text"]
    };
    let schema = if kind == "image" {
        crate::image_parameters::schema(&limits)
    } else {
        json!({"type":"object","required":["model","prompt"],"properties":{"model":{"type":"string"},"prompt":{"type":"string"},"size":{"type":"string"}}})
    };
    let mut capabilities = json!({"input_modalities":modalities,"output_modalities":[kind],"endpoints":endpoints,"limits":limits,"streaming":kind=="image","input_schema":schema});
    if let Some(declared) = model.extra.get("capabilities").and_then(Value::as_object) {
        capabilities
            .as_object_mut()
            .unwrap()
            .extend(declared.clone());
    }
    capabilities
}

/// 管理接口由统一管理鉴权中间件保护，不使用下游 API Key。
pub(crate) async fn account_models(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    query: web::Query<Query>,
) -> HttpResponse {
    let Some(account) = state.store.account_by_id(&path) else {
        return crate::protocol_api::error(404, "account_not_found", "账号不存在");
    };
    let failure = if query.refresh {
        refresh_account(&state, account.clone()).await.err()
    } else {
        None
    };
    let Some(manifest) = state.plugin_loader.get_plugin(&account.platform) else {
        return crate::protocol_api::error(404, "plugin_not_found", "插件不存在");
    };
    let cache = state
        .model_catalog
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let entry = cache.get(&account.id);
    let mut models = manifest
        .models
        .iter()
        .filter(|model| entry.is_none() || model.task_kind != mediagen_core::TaskKind::Text)
        .map(|model| model_value(manifest, model, &model.model_id))
        .collect::<Vec<_>>();
    if let Some(entry) = entry {
        models.extend(
            entry
                .models
                .iter()
                .map(|model| model_value(manifest, model, &model.model_id)),
        );
    }
    HttpResponse::Ok().json(json!({"data":{"models":models,"refreshed_at":entry.map(|entry| entry.refreshed_at),"refresh_error":failure,"discovery_supported":manifest.flows.contains_key("list_models")}}))
}

fn merge_capabilities(variants: &[Value]) -> Value {
    let mut merged = variants.first().cloned().unwrap_or(json!({}));
    for field in [
        "input_modalities",
        "output_modalities",
        "endpoints",
        "voices",
        "response_formats",
        "image_sources",
    ] {
        let mut values = Vec::new();
        for value in variants
            .iter()
            .flat_map(|variant| variant[field].as_array().into_iter().flatten())
        {
            if !values.contains(value) {
                values.push(value.clone());
            }
        }
        if !values.is_empty() {
            merged[field] = json!(values);
        }
    }
    for field in ["streaming", "tool_calling"] {
        if variants.iter().any(|variant| variant[field] == true) {
            merged[field] = json!(true);
        }
    }
    let unsupported = merged["unsupported_parameters"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|value| {
            variants.iter().all(|variant| {
                variant["unsupported_parameters"]
                    .as_array()
                    .is_some_and(|list| list.contains(value))
            })
        })
        .collect::<Vec<_>>();
    merged["unsupported_parameters"] = json!(unsupported);
    let mut efforts = Vec::new();
    for effort in variants.iter().flat_map(|variant| {
        variant["reasoning"]["efforts"]
            .as_array()
            .into_iter()
            .flatten()
    }) {
        if !efforts.contains(effort) {
            efforts.push(effort.clone());
        }
    }
    if !efforts.is_empty() {
        merged["reasoning"]["efforts"] = json!(efforts);
    }
    merged
}
