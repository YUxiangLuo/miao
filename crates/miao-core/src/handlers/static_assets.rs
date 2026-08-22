use axum::response::Html;

/// PWA 资源与面板一样全部嵌进二进制，保持单文件分发。
/// `no-cache` 保证每次发版后浏览器重新校验，SW 字节比对能即时触发更新，
/// 不会让旧版面板滞留（单文件 HTML 每个版本都变）。
const NO_CACHE: (axum::http::header::HeaderName, &str) =
    (axum::http::header::CACHE_CONTROL, "no-cache");

pub async fn serve_index() -> (
    [(axum::http::header::HeaderName, &'static str); 1],
    Html<&'static str>,
) {
    (
        [NO_CACHE],
        Html(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../public/index.html"
        ))),
    )
}

pub async fn serve_favicon() -> (
    [(axum::http::header::HeaderName, &'static str); 1],
    &'static str,
) {
    (
        [(axum::http::header::CONTENT_TYPE, "image/svg+xml")],
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../public/icon.svg"
        )),
    )
}

pub async fn serve_manifest() -> (
    [(axum::http::header::HeaderName, &'static str); 2],
    &'static str,
) {
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/manifest+json",
            ),
            NO_CACHE,
        ],
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../public/manifest.webmanifest"
        )),
    )
}

pub async fn serve_service_worker() -> (
    [(axum::http::header::HeaderName, &'static str); 2],
    &'static str,
) {
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "text/javascript; charset=utf-8",
            ),
            NO_CACHE,
        ],
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../public/sw.js")),
    )
}

macro_rules! png_handler {
    ($name:ident, $file:literal) => {
        pub async fn $name() -> (
            [(axum::http::header::HeaderName, &'static str); 1],
            &'static [u8],
        ) {
            (
                [(axum::http::header::CONTENT_TYPE, "image/png")],
                include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../public/", $file)),
            )
        }
    };
}

png_handler!(serve_icon_192, "icon-192.png");
png_handler!(serve_icon_512, "icon-512.png");
png_handler!(serve_icon_maskable_512, "icon-maskable-512.png");

#[cfg(test)]
mod tests {
    use super::{
        serve_favicon, serve_icon_192, serve_icon_512, serve_icon_maskable_512, serve_index,
        serve_manifest, serve_service_worker,
    };

    #[tokio::test]
    async fn serve_index_returns_html_document() {
        let (headers, axum::response::Html(html)) = serve_index().await;

        assert_eq!(headers[0].0, axum::http::header::CACHE_CONTROL);
        assert_eq!(headers[0].1, "no-cache");
        assert!(html.to_lowercase().contains("<!doctype html>"));
        assert!(html.contains("Miao 控制面板"));
    }

    #[tokio::test]
    async fn serve_index_links_the_web_app_manifest() {
        let (_, axum::response::Html(html)) = serve_index().await;

        // 构建工具可能规范化 manifest URL，只断言关键片段
        assert!(html.contains(r#"rel="manifest""#));
        assert!(html.contains("manifest.webmanifest"));
        assert!(html.contains(r#"name="theme-color""#));
    }

    #[tokio::test]
    async fn serve_favicon_returns_svg_content_type_and_body() {
        let (headers, body) = serve_favicon().await;

        assert_eq!(headers[0].1, "image/svg+xml");
        assert!(body.contains("<svg"));
    }

    #[tokio::test]
    async fn serve_manifest_returns_installable_manifest() {
        let (headers, body) = serve_manifest().await;

        assert_eq!(headers[0].1, "application/manifest+json");
        assert_eq!(headers[1].0, axum::http::header::CACHE_CONTROL);

        let manifest: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(manifest["display"], "fullscreen");
        assert_eq!(manifest["start_url"], "/");
        // Chrome 可安装门槛：192 与 512 PNG 图标
        let sizes: Vec<&str> = manifest["icons"]
            .as_array()
            .unwrap()
            .iter()
            .map(|icon| icon["sizes"].as_str().unwrap())
            .collect();
        assert!(sizes.contains(&"192x192"));
        assert!(sizes.contains(&"512x512"));
    }

    #[tokio::test]
    async fn serve_service_worker_returns_javascript_with_fetch_handler() {
        let (headers, body) = serve_service_worker().await;

        assert_eq!(headers[0].1, "text/javascript; charset=utf-8");
        assert_eq!(headers[1].0, axum::http::header::CACHE_CONTROL);
        // 可安装门槛：SW 必须带 fetch 监听；且不得拦截非导航请求（/api、WS）
        assert!(body.contains("addEventListener('fetch'"));
        assert!(body.contains("request.mode !== 'navigate'"));
    }

    async fn assert_png(
        (headers, body): (
            [(axum::http::header::HeaderName, &'static str); 1],
            &'static [u8],
        ),
    ) {
        assert_eq!(headers[0].1, "image/png");
        // PNG magic number
        assert_eq!(&body[..4], &[0x89, b'P', b'N', b'G']);
    }

    #[tokio::test]
    async fn serve_icons_return_png_bytes() {
        assert_png(serve_icon_192().await).await;
        assert_png(serve_icon_512().await).await;
        assert_png(serve_icon_maskable_512().await).await;
    }
}
