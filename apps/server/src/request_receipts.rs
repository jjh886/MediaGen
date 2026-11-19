//! 显式请求恢复的 Responses 调用。相同调用方和原请求号只允许受理一次。
use crate::{access_control::hash_secret, protocol_api::error, ServerState};
use actix_web::{web, HttpRequest, HttpResponse};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;

fn db(root: &Path) -> rusqlite::Result<Connection> {
    std::fs::create_dir_all(root).map_err(|_| rusqlite::Error::InvalidPath(root.to_owned()))?;
    let db = Connection::open(root.join("request-receipts.sqlite"))?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS receipts(owner TEXT NOT NULL, request TEXT NOT NULL, task TEXT UNIQUE NOT NULL, status TEXT NOT NULL DEFAULT 'running', result TEXT, expires INTEGER NOT NULL, PRIMARY KEY(owner,request)); CREATE INDEX IF NOT EXISTS receipts_expiry ON receipts(expires) WHERE status!='expired'")?;
    Ok(db)
}

pub(crate) async fn reserve(
    state: &ServerState,
    key: &str,
    id: &str,
    task: &str,
) -> Result<bool, String> {
    let root = state.file_root.clone();
    let (owner, id, task) = (hash_secret(key), id.to_owned(), task.to_owned());
    web::block(move || {
        db(&root)?.execute(
            "INSERT INTO receipts(owner,request,task,expires) VALUES(?1,?2,?3,unixepoch()+86400) ON CONFLICT DO NOTHING",
            params![owner, id, task],
        )
    })
    .await.map_err(|_| "回执写入中断")?
    .map(|changed| changed == 1).map_err(|_| "无法登记请求回执".into())
}

pub(crate) async fn finish(
    state: &web::Data<ServerState>,
    task: &str,
    status: &str,
    result: Option<&Value>,
) -> Result<(), String> {
    let state = state.clone();
    let task = task.to_string();
    let status = status.to_string();
    let result = result.cloned();
    web::block(move || {
        let encrypted = result
            .map(|v| state.credential_cipher.encrypt(&v.to_string()))
            .transpose()?;
        db(&state.file_root)
            .and_then(|db| {
                db.execute(
                    "UPDATE receipts SET status=?2,result=?3,expires=unixepoch()+86400 WHERE task=?1 AND status='running'",
                    params![task, status, encrypted],
                )
            })
            .map_err(|_| "无法保存请求结果".to_string())?;
        Ok(())
    })
    .await
    .map_err(|_| "回执保存中断")?
}

pub(crate) async fn fail(state: &web::Data<ServerState>, task: &str) {
    if let Err(message) = finish(state, task, "failed", None).await {
        tracing::error!(%message, "无法保存请求失败状态");
    }
}

pub(crate) async fn expire(state: &ServerState) -> Result<(), String> {
    let root = state.file_root.clone();
    web::block(move || {
        db(&root)?.execute(
            "UPDATE receipts SET status='expired',result=NULL WHERE expires<=unixepoch() AND status!='expired'",
            [],
        )
    })
    .await.map_err(|_| "回执清理中断")?
    .map(|_| ()).map_err(|_| "回执清理失败".into())
}

pub(crate) async fn lookup(
    state: web::Data<ServerState>,
    req: HttpRequest,
    path: web::Path<String>,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &req) {
        return error(401, "invalid_api_key", "API Key 无效");
    }
    let owner = hash_secret(&crate::routes::api_key_from_request(&req).unwrap_or_default());
    let Ok(id) = uuid::Uuid::parse_str(&path) else {
        return error(400, "invalid_request_id", "原请求编号无效");
    };
    let id = id.to_string();
    let request = id.clone();
    let row = web::block(move || -> Result<Option<Value>, String> {
        let db = db(&state.file_root).map_err(|_| "读取请求回执失败")?;
        let read = || db.query_row(
            "SELECT task,CASE WHEN expires<=unixepoch() THEN 'expired' ELSE status END,
             CASE WHEN expires>unixepoch() THEN result END FROM receipts WHERE owner=?1 AND request=?2",
            params![owner, id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?)),
        ).optional().map_err(|_| "读取请求回执失败");
        let Some((task, mut status, mut encrypted)) = read()? else { return Ok(None) };
        // 服务重启或执行器退出可先结束任务，来不及写入回执。只修正原调用方的原任务。
        if status == "running" && state.store.tasks.lock().unwrap_or_else(|e| e.into_inner())
            .iter().any(|t| t.id == task && matches!(t.status.as_str(), "failed" | "cancelled")) {
            db.execute("UPDATE receipts SET status='failed' WHERE owner=?1 AND request=?2 AND task=?3 AND status='running' AND expires>unixepoch()",
                params![owner,id,task]).map_err(|_| "请求回执核对失败")?;
            let Some((_, confirmed, result)) = read()? else { return Ok(None) };
            status = confirmed;
            encrypted = result;
        }
        let response = encrypted.map(|v| {
            let clear = state.credential_cipher.decrypt(&v)?;
            serde_json::from_str::<Value>(&clear).map_err(|_| "请求回执损坏".to_string())
        }).transpose()?;
        Ok(Some(json!({"request_id": request, "status": status, "response": response})))
    }).await;
    match row {
        Ok(Ok(Some(data))) => HttpResponse::Ok()
            .insert_header(("cache-control", "no-store"))
            .json(data),
        Ok(Ok(None)) => error(404, "request_not_found", "未找到原请求"),
        _ => error(500, "receipt_unavailable", "读取回执失败"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, App};
    #[actix_web::test]
    async fn disconnected_stream_finishes_without_another_generation() {
        let state = crate::protocol_api_tests::state();
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .configure(crate::configure_routes),
        )
        .await;
        let id = uuid::Uuid::new_v4().to_string();
        let body = json!({"model":"text-model","input":"original","store":false,"stream":true});
        let submit = || {
            test::TestRequest::post()
                .uri("/v1/responses")
                .insert_header(("authorization", "Bearer key-a"))
                .insert_header(("x-client-request-id", id.as_str()))
                .insert_header(("x-mediagen-receipt", "1"))
                .set_json(&body)
                .to_request()
        };
        let stream = test::call_service(&app, submit()).await;
        assert_eq!(stream.status(), 200);
        drop(stream);
        let uri = format!("/v1/response-requests/{id}");
        let receipt = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let value: Value = test::call_and_read_body_json(
                    &app,
                    test::TestRequest::get()
                        .uri(&uri)
                        .insert_header(("authorization", "Bearer key-a"))
                        .to_request(),
                )
                .await;
                if value["status"] != "running" {
                    break value;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(receipt["status"], "completed");
        assert_eq!(
            receipt["response"]["output"][0]["content"][0]["text"],
            "你好"
        );
        assert_eq!(test::call_service(&app, submit()).await.status(), 409);
        assert_eq!(
            state
                .store
                .list_tasks(None)
                .iter()
                .filter(|task| task.id.starts_with("receipt_"))
                .count(),
            1
        );
    }

    #[actix_web::test]
    async fn lookup_uses_original_request_and_scopes_receipts_to_the_caller() {
        let state = crate::protocol_api_tests::state();
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .configure(crate::configure_routes),
        )
        .await;
        let id = uuid::Uuid::new_v4().to_string();
        let input = json!({"model":"text-model","input":"first","store":false});
        let submit = || {
            test::TestRequest::post()
                .uri("/v1/responses")
                .insert_header(("authorization", "Bearer key-a"))
                .insert_header(("x-client-request-id", id.as_str()))
                .insert_header(("x-mediagen-receipt", "1"))
                .set_json(&input)
                .to_request()
        };
        let res = test::call_service(&app, submit()).await;
        assert_eq!(res.status(), 200);
        let original: Value = test::read_body_json(res).await;
        assert_eq!(test::call_service(&app, submit()).await.status(), 409);
        let uri = format!("/v1/response-requests/{id}");
        let get = |key: &str| {
            test::TestRequest::get()
                .uri(&uri)
                .insert_header(("authorization", format!("Bearer {key}")))
                .to_request()
        };
        let receipt: Value = test::call_and_read_body_json(&app, get("key-a")).await;
        assert_eq!(receipt["request_id"], id);
        assert_eq!(receipt["status"], "completed");
        assert_eq!(receipt["response"], original);
        assert_eq!(test::call_service(&app, get("key-b")).await.status(), 404);
        // 模拟重启打断了回执写入，但任务表保存了确定的失败。
        let db_conn = db(&state.file_root).unwrap();
        let task: String = db_conn
            .query_row("SELECT task FROM receipts WHERE request=?1", [&id], |r| {
                r.get(0)
            })
            .unwrap();
        state
            .store
            .fail_task(&task, "请求中断或客户端取消".into())
            .unwrap();
        db_conn
            .execute(
                "UPDATE receipts SET status='running',result=NULL WHERE request=?1",
                [&id],
            )
            .unwrap();
        let failed: Value = test::call_and_read_body_json(&app, get("key-a")).await;
        assert_eq!(failed["status"], "failed");
        assert!(failed["response"].is_null());
        assert_eq!(test::call_service(&app, submit()).await.status(), 409);
        assert_eq!(
            db_conn
                .query_row("SELECT status FROM receipts WHERE request=?1", [&id], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "failed"
        );
        db(&state.file_root)
            .unwrap()
            .execute("UPDATE receipts SET expires=0", [])
            .unwrap();
        expire(&state).await.unwrap();
        let expired: Value = test::call_and_read_body_json(&app, get("key-a")).await;
        assert_eq!(expired["status"], "expired");
        assert!(expired["response"].is_null());
        assert_eq!(test::call_service(&app, submit()).await.status(), 409);
    }
}
