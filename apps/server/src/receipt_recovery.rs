// 重启时先认领上一个进程的回执，再开放 HTTP 监听。查询回执不会触发生成。
use super::*;
use std::time::Duration;

pub(crate) async fn resume_after_restart(state: web::Data<ServerState>) -> Result<(), String> {
    let root = state.file_root.clone();
    web::block(move || {
        let mut conn = db(&root)?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let has_started: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('receipt_context') WHERE name='recovery_started')",
            [], |r| r.get(0),
        )?;
        if !has_started {
            tx.execute("ALTER TABLE receipt_context ADD COLUMN recovery_started INTEGER", [])?;
        }
        tx.execute("UPDATE receipts SET status='recovering' WHERE status IN ('running','failed') AND expires>unixepoch() AND task IN (SELECT task FROM receipt_context)", [])?;
        tx.execute("UPDATE receipt_context SET recovery_started=unixepoch() WHERE recovery_started IS NULL AND task IN (SELECT task FROM receipts WHERE status='recovering')", [])?;
        tx.commit()
    })
    .await.map_err(|_| "回执恢复初始化中断")?.map_err(|_| "无法认领待恢复回执")?;
    actix_web::rt::spawn(async move {
        loop {
            match recover_pass(&state).await {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) => tracing::warn!(%error,"读取待恢复回执失败"),
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
    Ok(())
}

async fn recover_pass(state: &web::Data<ServerState>) -> Result<usize, String> {
    let root = state.file_root.clone();
    let rows = web::block(move || {
        let conn = db(&root)?;
        let mut statement = conn.prepare(
            "SELECT r.task, r.owner, unixepoch()-c.recovery_started
             FROM receipts r JOIN receipt_context c ON c.task=r.task
             WHERE r.status='recovering' AND r.expires>unixepoch() ORDER BY r.expires",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })
    .await
    .map_err(|_| "回执恢复查询中断")?
    .map_err(|_| "回执恢复查询失败")?;
    let count = rows.len();
    use futures_util::{stream, StreamExt};
    stream::iter(rows)
        .for_each_concurrent(4, |(task, owner, age)| async move {
            if let Err(error) = recover_one(state, &task, &owner).await {
                // 错误可能带有第三方诊断信息，不向日志写入原文。
                let failure = if error.contains("receipt_terminal:") {
                    Some("原轮次缺少可恢复的完整响应")
                } else if age >= 20 * 60 {
                    Some("恢复等待超时，未取回原轮次的完整回复")
                } else {
                    None
                };
                if let Some(reason) = failure {
                    if !state
                        .store
                        .task_by_id(&task)
                        .is_some_and(|t| t.status == "completed")
                    {
                        state.store.fail_task(&task, reason.into());
                    }
                    super::fail(state, &task).await
                } else {
                    tracing::debug!(%task,"原回执仍待恢复");
                }
            }
        })
        .await;
    Ok(count)
}

async fn recover_one(state: &web::Data<ServerState>, id: &str, owner: &str) -> Result<(), String> {
    let root = state.file_root.clone();
    let task_id = id.to_owned();
    let encrypted: String = web::block(move || {
        db(&root)?.query_row(
            "SELECT ciphertext FROM receipt_context WHERE task=?1",
            [task_id],
            |r| r.get(0),
        )
    })
    .await
    .map_err(|_| "读取回执上下文中断")?
    .map_err(|_| "读取回执上下文失败")?;
    let ctx: Value = serde_json::from_str(&state.credential_cipher.decrypt(&encrypted)?)
        .map_err(|_| "恢复上下文损坏")?;
    let task = state
        .store
        .task_by_id(id)
        .ok_or("receipt_terminal: 未受理")?;
    if task.status == "cancelled" {
        return Err("receipt_terminal: 已取消".into());
    }
    let manifest = state
        .plugin_loader
        .get_plugin(&task.platform)
        .ok_or("插件未安装")?;
    let account = task
        .account_id
        .as_ref()
        .and_then(|id| state.store.account_by_id(id))
        .ok_or("receipt_terminal: 原账号不存在")?;
    let mut result = crate::protocol_task::run_plugin(
        state,
        manifest,
        &account,
        "recover_text_receipt",
        Value::Null,
        None,
        id,
        Value::Null,
    )
    .await?;
    if result["response"]["status"] != "completed"
        || result["response"]["object"] != "response"
        || !result["response"]["output"].is_array()
    {
        return Err("receipt_terminal: 原轮次没有完整响应".into());
    }
    let locks = crate::plugin_locks::PluginLocks::default();
    if let Some(session) = ctx["session"].as_str() {
        locks.acquire(&format!("conversation:{session}")).await?;
        let base = crate::conversation_affinity::begin_session(state, session, id).await?;
        crate::conversation_replay::save(
            state,
            session,
            &account.id,
            id,
            base.as_deref(),
            &ctx["input"],
            &result["response"],
        )
        .await?;
    }
    crate::conversation_affinity::redact_cursor(&mut result);
    let response = &mut result["response"];
    response["metadata"] = crate::protocol_storage::response_metadata(response, &ctx["metadata"]);
    response["store"] = ctx["store"].clone();
    response["previous_response_id"] = ctx["previous_response_id"].clone();
    let stored = if ctx["store"] == true {
        json!({"task_id":id,"owner":owner,"model_id":task.model_id,"input":ctx["input"],"conversation_session_id":ctx["session"],"response":response})
    } else {
        json!({"status":"completed","conversation_session_id":ctx["session"]})
    };
    let completed = state
        .store
        .complete_task(id, stored)
        .ok_or("receipt_terminal: 原任务已删除")?;
    if completed.status != "completed" {
        return Err("receipt_terminal: 原任务已取消".into());
    }
    if task.status != "completed" {
        state.store.mark_account_usage_for(&account.id, "text");
    }
    super::finish(state, id, "completed", Some(response)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, App};
    use std::sync::Arc;

    #[actix_web::test]
    async fn restart_migrates_older_context_table_without_dropping_records() {
        let state = crate::protocol_api_tests::state();
        let conn = db(&state.file_root).unwrap();
        conn.execute_batch("DROP TABLE receipt_context; CREATE TABLE receipt_context(task TEXT PRIMARY KEY,ciphertext TEXT NOT NULL); INSERT INTO receipt_context VALUES('old','original')").unwrap();
        resume_after_restart(state.clone()).await.unwrap();
        resume_after_restart(state).await.unwrap();
        assert_eq!(conn.query_row("SELECT ciphertext FROM receipt_context WHERE task='old' AND recovery_started IS NULL", [], |r| r.get::<_,String>(0)).unwrap(), "original");
    }

    #[actix_web::test]
    async fn restart_reads_original_turn_and_settles_only_original_owners_receipt() {
        let mut config = crate::protocol_api_tests::state().get_ref().clone();
        let dir = config.plugin_loader.get_plugin_dir("test").unwrap();
        let path = dir.join("manifest.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["flows"]["recover_text_receipt"] =
            json!({"steps":[{"action":"plugin.run_js","script_file":"read.js"}]});
        std::fs::write(path, manifest.to_string()).unwrap();
        std::fs::write(dir.join("scripts/read.js"),r#"function main(ctx){
          const saved=host.checkpoint();
          if (ctx.account_id!=="account" || saved.response_id!=="r_original") throw new Error("wrong turn");
          return {response:{id:"resp_original",object:"response",status:"completed",output:[{type:"message",role:"assistant",content:[{type:"output_text",text:"恢复原结果"}]}]}};
        }"#).unwrap();
        // 若恢复意外调用了生成入口，本测试会失败。
        std::fs::write(
            dir.join("scripts/text.js"),
            "function main(){throw new Error('unexpected new submission')}",
        )
        .unwrap();
        config.plugin_loader = Arc::new(
            mediagen_plugin_runtime::PluginLoader::load_from(dir.parent().unwrap()).unwrap(),
        );
        let root = config.store.path.parent().unwrap().to_owned();
        config.credential_cipher = Arc::new(crate::CredentialCipher::new(&root).unwrap());
        let task = "receipt_resume_restart_test";
        let request = uuid::Uuid::new_v4().to_string();
        let metadata = json!({"duanju_turn_id":"turn-42","duanju_round":"3"});
        let recovery = json!({"metadata":metadata,"store":false});
        assert!(reserve(&config, "key-a", &request, task, Some(recovery))
            .await
            .unwrap());
        let manifest = config.plugin_loader.get_plugin("test").unwrap();
        let (account, lease) = crate::protocol_task::reserve(
            &config,
            manifest,
            "text-model",
            "text",
            "key-a",
            task,
            None,
        )
        .unwrap();
        let host = crate::server_plugin_host::ServerPluginHost::new_for_account(&account)
            .with_storage(
                config.credential_cipher.clone(),
                &account.profile_dir,
                manifest,
            )
            .with_events(None, task.into());
        host.checkpoint(&json!({"value":{"phase":"accepted","response_id":"r_original"}}))
            .await
            .unwrap();
        drop(lease);
        config.store = Arc::new(crate::store::AppStore::load(config.store.path.clone()));
        config.credential_cipher = Arc::new(crate::CredentialCipher::new(&root).unwrap());
        let state = web::Data::new(config);
        resume_after_restart(state.clone()).await.unwrap();
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .configure(crate::configure_routes),
        )
        .await;
        let get = |key: &str| {
            test::TestRequest::get()
                .uri(&format!("/v1/response-requests/{request}"))
                .insert_header(("authorization", format!("Bearer {key}")))
                .to_request()
        };
        assert_eq!(test::call_service(&app, get("key-b")).await.status(), 404);
        let value = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let receipt: Value = test::call_and_read_body_json(&app, get("key-a")).await;
                if receipt["status"] != "running" {
                    break receipt;
                }
                tokio::time::sleep(Duration::from_millis(10)).await
            }
        })
        .await
        .unwrap();
        assert_eq!(value["status"], "completed");
        assert_eq!(value["response"]["id"], "resp_original");
        assert_eq!(value["response"]["metadata"], metadata);
        assert!(!value["response"]["store"].as_bool().unwrap());
        assert_eq!(
            value["response"]["output"][0]["content"][0]["text"],
            "恢复原结果"
        );
        let task_record = state.store.task_by_id(task).unwrap();
        assert_eq!(task_record.status, "completed");
        assert!(task_record.error_message.is_none());
        assert_eq!(recover_pass(&state).await.unwrap(), 0);
        assert!(!reserve(&state, "key-a", &request, task, None)
            .await
            .unwrap());
    }
}
