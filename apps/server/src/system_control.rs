use std::net::IpAddr;
use std::time::Duration;

use actix_web::{web, HttpRequest, HttpResponse, Responder};

use crate::ServerState;

fn exit_supported() -> bool {
    cfg!(target_os = "macos") || cfg!(target_os = "windows")
}

fn is_loopback_request(request: &HttpRequest) -> bool {
    request
        .peer_addr()
        .map(|addr| {
            matches!(addr.ip(), IpAddr::V4(ip) if ip.is_loopback())
                || matches!(addr.ip(), IpAddr::V6(ip) if ip.is_loopback())
        })
        .unwrap_or(false)
}

pub async fn system_info() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "data": {
            "os": std::env::consts::OS,
            "exit_supported": exit_supported()
        }
    }))
}

pub async fn exit_app(state: web::Data<ServerState>, request: HttpRequest) -> impl Responder {
    // 退出能力只服务 macOS/Windows 本机启动器，Linux 服务端默认常驻运行。
    if !exit_supported() {
        return HttpResponse::Forbidden().json(serde_json::json!({
            "error": "当前系统不支持从 Web 控制台退出 MediaGen"
        }));
    }

    if !is_loopback_request(&request) {
        return HttpResponse::Forbidden().json(serde_json::json!({
            "error": "只允许在运行 MediaGen 的本机退出应用"
        }));
    }

    let gateway = state.browser_gateway.clone();
    actix_web::rt::spawn(async move {
        actix_web::rt::time::sleep(Duration::from_millis(300)).await;
        if let Ok(sessions) = gateway.list_sessions() {
            for session in sessions {
                let _ = gateway.cancel_session(&session.id).await;
            }
        }
        std::process::exit(0);
    });

    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}
