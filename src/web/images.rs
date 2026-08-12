use axum::{
    extract::Path,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use std::path::{Component, PathBuf};

const IMAGES_DIR: &str = "/app/images";

pub async fn image_handler(Path(path): Path<String>) -> Response {
    // preview.png is reserved for social-media metadata.
    // It is served from /app/www/images/preview.png,
    // not from the persistent application image directory.
    if path == "preview.png" {
        return crate::web::pages::social_preview().await;
    }

    let relative = PathBuf::from(&path);

    // Prevent path traversal.
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return (StatusCode::BAD_REQUEST, "Invalid image path").into_response();
    }

    let image_path = PathBuf::from(IMAGES_DIR).join(relative);

    let content = match tokio::fs::read(&image_path).await {
        Ok(content) => content,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "Image not found").into_response();
        }
    };

    let content_type = match image_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("avif") => "image/avif",
        _ => "application/octet-stream",
    };

    let content_type = HeaderValue::from_static(content_type);

    (
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=86400"),
            ),
        ],
        content,
    )
        .into_response()
}
