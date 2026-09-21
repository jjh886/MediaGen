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
    let mut response = next.call(request).await?.map_into_left_body();
    response.headers_mut().insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_str(&id).expect("UUID header"),
    );
    Ok(response)
}
