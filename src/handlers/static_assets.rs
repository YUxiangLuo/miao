use axum::response::Html;

pub async fn serve_index() -> Html<&'static str> {
    Html(include_str!("../../public/index.html"))
}

pub async fn serve_favicon() -> (
    [(axum::http::header::HeaderName, &'static str); 1],
    &'static str,
) {
    (
        [(axum::http::header::CONTENT_TYPE, "image/svg+xml")],
        include_str!("../../public/icon.svg"),
    )
}

#[cfg(test)]
mod tests {
    use super::{serve_favicon, serve_index};

    #[tokio::test]
    async fn serve_index_returns_html_document() {
        let axum::response::Html(html) = serve_index().await;

        assert!(html.to_lowercase().contains("<!doctype html>"));
        assert!(html.contains("Miao 控制面板"));
    }

    #[tokio::test]
    async fn serve_favicon_returns_svg_content_type_and_body() {
        let (headers, body) = serve_favicon().await;

        assert_eq!(headers[0].1, "image/svg+xml");
        assert!(body.contains("<svg"));
    }
}
