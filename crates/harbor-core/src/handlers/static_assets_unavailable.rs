use axum::http::{header::CACHE_CONTROL, HeaderName, StatusCode};

pub async fn unavailable() -> (StatusCode, [(HeaderName, &'static str); 1], &'static str) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(CACHE_CONTROL, "no-store")],
        "Panel assets are not included in this build. Enable the embedded-assets feature.",
    )
}

pub use unavailable as serve_favicon;
pub use unavailable as serve_icon_192;
pub use unavailable as serve_icon_512;
pub use unavailable as serve_icon_maskable_512;
pub use unavailable as serve_index;
pub use unavailable as serve_manifest;
pub use unavailable as serve_service_worker;
