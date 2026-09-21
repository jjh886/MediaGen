use actix_files::NamedFile;
use actix_web::{web, HttpRequest};
use std::path::PathBuf;
pub(crate) struct ConsoleRoot(pub PathBuf);
pub(crate) async fn fallback(
    request: HttpRequest,
    root: web::Data<ConsoleRoot>,
) -> actix_web::Result<NamedFile> {
    let path = request.path();
    if !matches!(request.method().as_str(), "GET" | "HEAD")
        || path.starts_with("/api/")
        || path.starts_with("/v1/")
        || path.starts_with("/assets/")
        || path.rsplit('/').next().is_some_and(|p| p.contains('.'))
    {
        return Err(actix_web::error::ErrorNotFound("Not found"));
    }
    Ok(NamedFile::open_async(root.0.join("index.html")).await?)
}

#[cfg(test)]
mod tests {
    #[actix_web::test]
    async fn deep_links_serve_console_without_swallowing_api_or_assets() {
        use actix_web::{test as at, web, App};
        let root = std::env::temp_dir().join(format!("console-routing-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.html"), "<html>console</html>").unwrap();
        let app = at::init_service(
            App::new()
                .app_data(web::Data::new(super::ConsoleRoot(root.clone())))
                .service(
                    actix_files::Files::new("/", &root)
                        .index_file("index.html")
                        .default_handler(web::to(super::fallback)),
                ),
        )
        .await;
        for (path, status) in [
            ("/accounts", 200),
            ("/admin-users", 200),
            ("/logs?task_id=task", 200),
            ("/api/missing", 404),
            ("/assets/missing.js", 404),
        ] {
            let response =
                at::call_service(&app, at::TestRequest::get().uri(path).to_request()).await;
            assert_eq!(response.status(), status, "{path}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
