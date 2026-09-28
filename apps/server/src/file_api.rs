use std::cmp::Reverse;
use std::path::{Path, PathBuf};

use actix_multipart::{Field, Multipart};
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use bytes::BytesMut;
use chrono::Utc;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::access_control::hash_secret;
use crate::image_multipart::{field_filename, field_name};
use crate::routes::{api_key_allowed, api_key_from_request};
use crate::ServerState;

const MAX_FILE_BYTES: usize = 50 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FileRecord {
    pub id: String,
    pub object: String,
    pub bytes: u64,
    pub created_at: i64,
    pub filename: String,
    pub purpose: String,
    #[serde(default)]
    pub owner_hash: String,
}

pub async fn upload_file(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> impl Responder {
    if !api_key_allowed(&state, &request) {
        return file_error(401, "authentication_error", "invalid api key");
    }
    let owner_hash = owner_hash_from_request(&request);
    match parse_upload(payload, request).await {
        Ok(upload) => save_upload(&state.file_root, upload, owner_hash).await,
        Err((status, message)) => file_error(status, "invalid_request_error", &message),
    }
}

pub async fn list_files(state: web::Data<ServerState>, request: HttpRequest) -> impl Responder {
    if !api_key_allowed(&state, &request) {
        return file_error(401, "authentication_error", "invalid api key");
    }
    let owner_hash = owner_hash_from_request(&request);
    let query = match web::Query::<FileListQuery>::from_query(request.query_string()) {
        Ok(query)
            if query.limit.is_none_or(|limit| (1..=10000).contains(&limit))
                && query
                    .order
                    .as_deref()
                    .is_none_or(|order| matches!(order, "asc" | "desc")) =>
        {
            query.into_inner()
        }
        _ => return file_error(400, "invalid_request_error", "文件列表参数无效"),
    };
    match read_all_records(&state.file_root, &owner_hash).await {
        Ok(mut data) => {
            data.retain(|item| {
                query
                    .purpose
                    .as_ref()
                    .is_none_or(|purpose| &item.purpose == purpose)
            });
            data.sort_by_key(|item| Reverse((item.created_at, item.id.clone())));
            if query.order.as_deref() == Some("asc") {
                data.reverse();
            }
            if let Some(after) = query.after {
                let Some(index) = data.iter().position(|item| item.id == after) else {
                    return file_error(400, "invalid_request_error", "after 文件不存在或无权访问");
                };
                data.drain(..=index);
            }
            let limit = query.limit.unwrap_or(10000);
            let has_more = data.len() > limit;
            data.truncate(limit);
            HttpResponse::Ok().json(json!({
                "object": "list",
                "first_id": data.first().map(|item| &item.id),
                "last_id": data.last().map(|item| &item.id),
                "has_more": has_more,
                "data": data.into_iter().map(public_record).collect::<Vec<_>>()
            }))
        }
        Err(message) => file_error(500, "server_error", &message),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileListQuery {
    limit: Option<usize>,
    after: Option<String>,
    order: Option<String>,
    purpose: Option<String>,
}

pub async fn retrieve_file(
    state: web::Data<ServerState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    if !api_key_allowed(&state, &request) {
        return file_error(401, "authentication_error", "invalid api key");
    }
    let owner_hash = owner_hash_from_request(&request);
    match read_record_for_owner(&state.file_root, &path.into_inner(), &owner_hash).await {
        Ok(record) => HttpResponse::Ok().json(public_record(record)),
        Err(message) => file_error(404, "not_found", &message),
    }
}

pub async fn retrieve_file_content(
    state: web::Data<ServerState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    if !api_key_allowed(&state, &request) {
        return file_error(401, "authentication_error", "invalid api key");
    }
    let id = path.into_inner();
    let owner_hash = owner_hash_from_request(&request);
    if let Err(message) = read_record_for_owner(&state.file_root, &id, &owner_hash).await {
        return file_error(404, "not_found", &message);
    }
    match tokio::fs::read(content_path(&state.file_root, &id)).await {
        Ok(bytes) => HttpResponse::Ok().body(bytes),
        Err(err) => file_error(404, "not_found", &err.to_string()),
    }
}

pub async fn delete_file(
    state: web::Data<ServerState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    if !api_key_allowed(&state, &request) {
        return file_error(401, "authentication_error", "invalid api key");
    }
    let id = path.into_inner();
    if let Err(message) = validate_file_id(&id) {
        return file_error(400, "invalid_file_id", &message);
    }
    let owner_hash = owner_hash_from_request(&request);
    let existed = read_record_for_owner(&state.file_root, &id, &owner_hash)
        .await
        .is_ok();
    if !existed {
        return file_error(404, "not_found", "文件不存在");
    }
    let _ = tokio::fs::remove_file(content_path(&state.file_root, &id)).await;
    let _ = tokio::fs::remove_file(metadata_path(&state.file_root, &id)).await;
    HttpResponse::Ok().json(json!({ "id": id, "object": "file", "deleted": existed }))
}

pub(crate) async fn read_file_as_data_url(
    root: &Path,
    id: &str,
    owner_hash: &str,
) -> Result<String, String> {
    let record = read_record_for_owner(root, id, owner_hash).await?;
    let bytes = tokio::fs::read(content_path(root, id))
        .await
        .map_err(|err| err.to_string())?;
    let mime = mime_from_filename(&record.filename);
    Ok(format!(
        "data:{mime};base64,{}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
    ))
}

struct UploadInput {
    filename: String,
    purpose: String,
    bytes: bytes::Bytes,
}

async fn parse_upload(
    payload: web::Payload,
    request: HttpRequest,
) -> Result<UploadInput, (u16, String)> {
    let mut multipart = Multipart::new(request.headers(), payload);
    let mut purpose = "assistants".to_string();
    let mut file = None;
    let mut total = 0usize;
    while let Some(item) = multipart.next().await {
        let mut field = item.map_err(|err| (400, err.to_string()))?;
        let name = field_name(&field);
        let filename = field_filename(&field);
        let bytes = read_field(&mut field, &mut total).await?;
        if name == "purpose" {
            purpose = String::from_utf8_lossy(&bytes).trim().to_string();
        } else if name == "file" {
            file = Some(UploadInput {
                filename: sanitize_filename(filename.as_deref().unwrap_or("file.bin")),
                purpose: purpose.clone(),
                bytes,
            });
        }
    }
    let mut upload = file.ok_or_else(|| (400, "缺少 file 字段".to_string()))?;
    upload.purpose = purpose;
    Ok(upload)
}

async fn save_upload(root: &Path, upload: UploadInput, owner_hash: String) -> HttpResponse {
    if let Err(err) = tokio::fs::create_dir_all(root).await {
        return file_error(500, "server_error", &err.to_string());
    }
    let id = format!("file-{}", Uuid::new_v4());
    let record = FileRecord {
        id: id.clone(),
        object: "file".to_string(),
        bytes: upload.bytes.len() as u64,
        created_at: Utc::now().timestamp(),
        filename: upload.filename,
        purpose: upload.purpose,
        owner_hash,
    };
    if let Err(err) = tokio::fs::write(content_path(root, &id), upload.bytes).await {
        return file_error(500, "server_error", &err.to_string());
    }
    if let Err(err) = write_record(root, &record).await {
        return file_error(500, "server_error", &err);
    }
    HttpResponse::Ok().json(public_record(record))
}

async fn read_all_records(root: &Path, owner_hash: &str) -> Result<Vec<FileRecord>, String> {
    let mut out = Vec::new();
    let mut entries = match tokio::fs::read_dir(root).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(err) => return Err(err.to_string()),
    };
    while let Some(entry) = entries.next_entry().await.map_err(|err| err.to_string())? {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("json") {
            let record = read_record_path(&path).await?;
            if owner_can_access(&record, owner_hash) {
                out.push(record);
            }
        }
    }
    Ok(out)
}

async fn read_record(root: &Path, id: &str) -> Result<FileRecord, String> {
    validate_file_id(id)?;
    read_record_path(&metadata_path(root, id)).await
}

pub(crate) async fn read_record_for_owner(
    root: &Path,
    id: &str,
    owner_hash: &str,
) -> Result<FileRecord, String> {
    let record = read_record(root, id).await?;
    if owner_can_access(&record, owner_hash) {
        Ok(record)
    } else {
        Err("文件不存在或无权访问".to_string())
    }
}

async fn read_record_path(path: &Path) -> Result<FileRecord, String> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|err| err.to_string())?;
    serde_json::from_str(&content).map_err(|err| err.to_string())
}

async fn write_record(root: &Path, record: &FileRecord) -> Result<(), String> {
    let content = serde_json::to_vec_pretty(record).map_err(|err| err.to_string())?;
    tokio::fs::write(
        metadata_path(root, &record.id),
        [&content[..], b"\n"].concat(),
    )
    .await
    .map_err(|err| err.to_string())
}

async fn read_field(field: &mut Field, total: &mut usize) -> Result<bytes::Bytes, (u16, String)> {
    let mut body = BytesMut::new();
    while let Some(chunk) = field.next().await {
        let chunk = chunk.map_err(|err| (400, err.to_string()))?;
        *total += chunk.len();
        if *total > MAX_FILE_BYTES {
            return Err((413, "文件不能超过 50MB".to_string()));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body.freeze())
}

fn content_path(root: &Path, id: &str) -> PathBuf {
    root.join(format!("{id}.bin"))
}

fn metadata_path(root: &Path, id: &str) -> PathBuf {
    root.join(format!("{id}.json"))
}

fn validate_file_id(id: &str) -> Result<(), String> {
    let body = id
        .strip_prefix("file-")
        .ok_or_else(|| "file_id 必须以 file- 开头".to_string())?;
    let valid = body.len() >= 32
        && body
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err("file_id 格式不正确".to_string())
    }
}

fn owner_hash_from_request(request: &HttpRequest) -> String {
    api_key_from_request(request)
        .map(|key| hash_secret(&key))
        .unwrap_or_default()
}

fn owner_can_access(record: &FileRecord, owner_hash: &str) -> bool {
    !owner_hash.is_empty() && record.owner_hash == owner_hash
}

fn public_record(record: FileRecord) -> serde_json::Value {
    json!({
        "id": record.id,
        "object": record.object,
        "bytes": record.bytes,
        "created_at": record.created_at,
        "filename": record.filename,
        "purpose": record.purpose,
    })
}

fn sanitize_filename(value: &str) -> String {
    let name = Path::new(value)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("file.bin")
        .trim();
    if name.is_empty() {
        "file.bin".to_string()
    } else {
        name.to_string()
    }
}

pub(crate) fn mime_from_filename(filename: &str) -> &'static str {
    match Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "png" => "image/png",
        "pdf" => "application/pdf",
        "txt" | "md" => "text/plain",
        "json" => "application/json",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "ogg" | "opus" => "audio/ogg",
        "m4a" => "audio/mp4",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "csv" => "text/csv",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "py" | "js" | "jsx" | "ts" | "tsx" | "go" | "rs" | "java" | "c" | "cpp" | "h" | "cs"
        | "php" | "rb" | "sh" | "sql" | "html" | "css" | "xml" | "yaml" | "yml" | "toml"
        | "log" => "text/plain",
        _ => "application/octet-stream",
    }
}

fn file_error(status: u16, code: &'static str, message: &str) -> HttpResponse {
    crate::protocol_api::error(
        status,
        code,
        if status >= 500 {
            "文件操作失败"
        } else {
            message
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{mime_from_filename, sanitize_filename};

    #[test]
    fn sanitizes_filename_to_basename() {
        assert_eq!(sanitize_filename("../a.png"), "a.png");
        assert_eq!(sanitize_filename(" "), "file.bin");
    }

    #[test]
    fn infers_image_mime_from_filename() {
        assert_eq!(mime_from_filename("a.jpg"), "image/jpeg");
        assert_eq!(mime_from_filename("a.webp"), "image/webp");
        assert_eq!(mime_from_filename("a.png"), "image/png");
        assert_eq!(mime_from_filename("a.pdf"), "application/pdf");
        assert_eq!(mime_from_filename("a.unknown"), "application/octet-stream");
    }

    #[test]
    fn validates_file_id_before_path_join() {
        assert!(super::validate_file_id("file-123e4567-e89b-12d3-a456-426614174000").is_ok());
        assert!(super::validate_file_id("../secret").is_err());
        assert!(super::validate_file_id("file-../../secret").is_err());
    }
}
