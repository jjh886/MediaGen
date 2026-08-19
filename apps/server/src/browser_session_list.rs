use std::cmp::Reverse;

use actix_web::{web, HttpResponse, Responder};
use mediagen_browser_gateway::BrowserSessionPurpose;
use serde::Deserialize;
use serde_json::json;

use crate::browser_session_public::public_browser_sessions;
use crate::browser_sessions::gateway_error_response;
use crate::pagination::{paginate, PageQuery};
use crate::ServerState;

#[derive(Default, Deserialize)]
pub(crate) struct BrowserSessionListQuery {
    #[serde(default)]
    takeover_only: bool,
    task_id: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

pub(crate) async fn list_browser_sessions(
    state: web::Data<ServerState>,
    query: web::Query<BrowserSessionListQuery>,
) -> impl Responder {
    match state.browser_gateway.list_sessions() {
        Ok(mut sessions) => {
            if query.takeover_only {
                sessions.retain(|session| {
                    matches!(
                        session.purpose,
                        BrowserSessionPurpose::HumanVerification
                            | BrowserSessionPurpose::TaskTakeover
                    )
                });
            }
            if let Some(task_id) = query
                .task_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
            {
                sessions.retain(|session| session.task_id.as_deref() == Some(task_id));
            }
            sessions.sort_by_key(|session| Reverse(session.created_at));
            let page = paginate(sessions, &PageQuery::new(query.limit, query.offset));
            HttpResponse::Ok().json(json!({ "data": public_browser_sessions(page) }))
        }
        Err(err) => gateway_error_response(err),
    }
}
