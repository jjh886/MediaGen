use actix_web::{
    body::{EitherBody, MessageBody},
    dev::{ServiceRequest, ServiceResponse},
    http::header::{HeaderName, HeaderValue},
    middleware::Next,
    Error, HttpMessage, HttpRequest,
};

#[derive(Clone)]
struct RequestId(String);

pub(crate) fn id(request: &HttpRequest) -> String {
    request
        .extensions()
        .get::<RequestId>()
        .map(|value| value.0.clone())
        .unwrap_or_else(|| format!("req_{}", uuid::Uuid::new_v4().simple()))
}

pub(crate) fn new_turn(request: &HttpRequest) {
    request
        .extensions_mut()
        .insert(RequestId(format!("req_{}", uuid::Uuid::new_v4().simple())));
}

pub(crate) async fn middleware(
    request: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<EitherBody<impl MessageBody>>, Error> {
    let id = format!("req_{}", uuid::Uuid::new_v4().simple());
    if let Some(value) = request.headers().get("x-client-request-id") {
        if value.to_str().ok().is_none_or(|value| {
            value.is_empty()
                || value.len() > 512
                || !value.bytes().all(|byte| (32..=126).contains(&byte))
        }) {
            let mut response = crate::protocol_api::error(
                400,
                "invalid_request_id",
                "X-Client-Request-Id 必须为 1–512 个可打印 ASCII 字符",
            );
            response.headers_mut().insert(
                HeaderName::from_static("x-request-id"),
                HeaderValue::from_str(&id).expect("UUID header"),
            );
            return Ok(request.into_response(response).map_into_right_body());
        }
    }
    request.extensions_mut().insert(RequestId(id.clone()));
    let started = std::time::Instant::now();
    let method = request.method().as_str().to_owned();
    let path = request.path().chars().take(256).collect::<String>();
    // 记录路由与请求标识，不记录查询参数、请求头或正文。日志不写入业务快照。
    eprintln!(
        "{}",
        serde_json::json!({
            "event": "request_started", "request_id": id, "method": method, "path": path
        })
    );
    let result = next.call(request).await;
    let status = match &result {
        Ok(response) => response.status(),
        Err(error) => error.as_response_error().status_code(),
    };
    eprintln!(
        "{}",
        serde_json::json!({
            "event": "response_headers_ready", "request_id": id, "method": method,
            "path": path, "status": status.as_u16(), "handler_ms": started.elapsed().as_millis()
        })
    );
    let mut response = result?.map_into_left_body();
    response.headers_mut().insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_str(&id).expect("UUID header"),
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, web, App, HttpResponse};

    #[actix_web::test]
    async fn tracing_preserves_status_and_request_id() {
        let app = test::init_service(
            App::new()
                .wrap(actix_web::middleware::from_fn(middleware))
                .route(
                    "/probe",
                    web::get().to(|request: HttpRequest| async move {
                        HttpResponse::Accepted().json(serde_json::json!({"id": id(&request)}))
                    }),
                ),
        )
        .await;
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/probe?private=not-logged")
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), actix_web::http::StatusCode::ACCEPTED);
        let request_id = response
            .headers()
            .get("x-request-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let body: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(body["id"], request_id);
    }
}
