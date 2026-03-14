use axum::response::Html;

pub async fn serve_index() -> Html<&'static str> {
    Html(include_str!("../../public/index.html"))
}

pub async fn serve_favicon() -> ([(axum::http::header::HeaderName, &'static str); 1], &'static str) {
    ([(axum::http::header::CONTENT_TYPE, "image/svg+xml")], include_str!("../../public/icon.svg"))
}

pub async fn serve_vue() -> ([(axum::http::header::HeaderName, &'static str); 1], &'static str) {
    ([(axum::http::header::CONTENT_TYPE, "application/javascript")], include_str!("../../embedded/assets/vue.js"))
}

pub async fn serve_bootstrap_css() -> ([(axum::http::header::HeaderName, &'static str); 1], &'static str) {
    ([(axum::http::header::CONTENT_TYPE, "text/css")], include_str!("../../embedded/assets/bootstrap-icons.css"))
}

pub async fn serve_font_woff2() -> ([(axum::http::header::HeaderName, &'static str); 1], &'static [u8]) {
    ([(axum::http::header::CONTENT_TYPE, "font/woff2")], include_bytes!("../../embedded/assets/fonts/bootstrap-icons.woff2"))
}

pub async fn serve_font_woff() -> ([(axum::http::header::HeaderName, &'static str); 1], &'static [u8]) {
    ([(axum::http::header::CONTENT_TYPE, "font/woff")], include_bytes!("../../embedded/assets/fonts/bootstrap-icons.woff"))
}

#[cfg(test)]
mod tests {
    use super::{serve_favicon, serve_index, serve_vue};

    #[tokio::test]
    async fn serve_index_returns_html_document() {
        let axum::response::Html(html) = serve_index().await;

        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Miao 控制面板"));
    }

    #[tokio::test]
    async fn serve_favicon_returns_svg_content_type_and_body() {
        let (headers, body) = serve_favicon().await;

        assert_eq!(headers[0].1, "image/svg+xml");
        assert!(body.contains("<svg"));
    }

    #[tokio::test]
    async fn serve_vue_returns_javascript_asset() {
        let (headers, body) = serve_vue().await;

        assert_eq!(headers[0].1, "application/javascript");
        assert!(!body.is_empty());
    }
}
