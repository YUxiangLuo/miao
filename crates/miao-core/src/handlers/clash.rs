use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        OriginalUri, State,
    },
    http::{header, HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use futures::{SinkExt, StreamExt};
use tokio::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message as TungsteniteMessage};
use tracing::warn;

use crate::services::singbox::{CLASH_API_BASE, CLASH_TRAFFIC_WS};
use crate::state::AppState;

fn clash_target_url(uri: &axum::http::Uri) -> String {
    let path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let suffix = path_and_query
        .strip_prefix("/api/clash")
        .unwrap_or(path_and_query);
    let suffix = if suffix.is_empty() { "/" } else { suffix };
    format!("{CLASH_API_BASE}{suffix}")
}

fn share_connections(
    method: &Method,
    uri: &axum::http::Uri,
    headers: &HeaderMap,
    body: &Bytes,
) -> bool {
    method == Method::GET
        && uri.path() == "/api/clash/connections"
        && uri.query().is_none()
        && !headers.contains_key(header::CONTENT_TYPE)
        && body.is_empty()
}

pub async fn proxy_clash_http(
    State(state): State<Arc<AppState>>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let target = clash_target_url(&uri);
    if share_connections(&method, &uri, &headers, &body) {
        return match state
            .connection_requests
            .get(
                &state.http_client,
                &target,
                state.lifecycle.snapshot().generation,
            )
            .await
        {
            Ok(snapshot) => {
                let mut response = Response::new(axum::body::Body::from(snapshot.body));
                *response.status_mut() = snapshot.status;
                if let Some(content_type) = snapshot.content_type {
                    response
                        .headers_mut()
                        .insert(header::CONTENT_TYPE, content_type);
                }
                response
            }
            Err(err) => {
                warn!(error = %err, "Failed to proxy Clash connections request");
                StatusCode::BAD_GATEWAY.into_response()
            }
        };
    }
    let reqwest_method = match reqwest::Method::from_bytes(method.as_str().as_bytes()) {
        Ok(method) => method,
        Err(_) => return StatusCode::METHOD_NOT_ALLOWED.into_response(),
    };

    let mut request = state
        .http_client
        .request(reqwest_method, target)
        .timeout(Duration::from_secs(10));

    if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
        request = request.header(header::CONTENT_TYPE.as_str(), content_type);
    }
    if !body.is_empty() {
        request = request.body(body);
    }

    let mutates = method != Method::GET && method != Method::HEAD;
    if mutates {
        state.connection_requests.invalidate();
    }
    let response = match request.send().await {
        Ok(response) => {
            let status =
                StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
            match response.bytes().await {
                Ok(bytes) => {
                    let mut builder = Response::builder().status(status);
                    if let Some(content_type) = content_type {
                        builder = builder.header(header::CONTENT_TYPE, content_type);
                    }
                    builder
                        .body(axum::body::Body::from(bytes))
                        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
                }
                Err(err) => {
                    warn!(error = %err, "Failed to read Clash API response");
                    StatusCode::BAD_GATEWAY.into_response()
                }
            }
        }
        Err(err) => {
            warn!(error = %err, "Failed to proxy Clash API request");
            StatusCode::BAD_GATEWAY.into_response()
        }
    };
    if mutates {
        state.connection_requests.invalidate();
    }
    response
}

pub async fn proxy_clash_traffic(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(bridge_traffic_socket)
}

async fn bridge_traffic_socket(socket: WebSocket) {
    let upstream = match connect_async(CLASH_TRAFFIC_WS).await {
        Ok((socket, _)) => socket,
        Err(err) => {
            warn!(error = %err, "Failed to connect to Clash traffic WebSocket");
            return;
        }
    };

    let (mut client_tx, mut client_rx) = socket.split();
    let (mut upstream_tx, mut upstream_rx) = upstream.split();

    loop {
        tokio::select! {
            upstream_msg = upstream_rx.next() => {
                let Some(upstream_msg) = upstream_msg else { break };
                let Ok(upstream_msg) = upstream_msg else { break };
                let client_msg = match upstream_msg {
                    TungsteniteMessage::Text(text) => Message::Text(text.to_string().into()),
                    TungsteniteMessage::Binary(bytes) => Message::Binary(bytes),
                    TungsteniteMessage::Ping(bytes) => Message::Ping(bytes),
                    TungsteniteMessage::Pong(bytes) => Message::Pong(bytes),
                    TungsteniteMessage::Close(_) => break,
                    TungsteniteMessage::Frame(_) => continue,
                };
                if client_tx.send(client_msg).await.is_err() {
                    break;
                }
            }
            client_msg = client_rx.next() => {
                let Some(client_msg) = client_msg else { break };
                let Ok(client_msg) = client_msg else { break };
                let upstream_msg = match client_msg {
                    Message::Text(text) => TungsteniteMessage::Text(text.to_string().into()),
                    Message::Binary(bytes) => TungsteniteMessage::Binary(bytes),
                    Message::Ping(bytes) => TungsteniteMessage::Ping(bytes),
                    Message::Pong(bytes) => TungsteniteMessage::Pong(bytes),
                    Message::Close(_) => {
                        let _ = upstream_tx.send(TungsteniteMessage::Close(None)).await;
                        break;
                    }
                };
                if upstream_tx.send(upstream_msg).await.is_err() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_connection_reads_are_shared() {
        let uri = "/api/clash/connections".parse().unwrap();
        let headers = HeaderMap::new();
        let body = Bytes::new();
        assert!(share_connections(&Method::GET, &uri, &headers, &body));
        for method in [Method::DELETE, Method::POST, Method::HEAD] {
            assert!(!share_connections(&method, &uri, &headers, &body));
        }
        for path in [
            "/api/clash/connections?",
            "/api/clash/connections?foo=1",
            "/api/clash/connections/abc",
            "/api/clash/traffic",
        ] {
            assert!(!share_connections(
                &Method::GET,
                &path.parse().unwrap(),
                &headers,
                &body
            ));
        }
        assert!(!share_connections(
            &Method::GET,
            &uri,
            &headers,
            &Bytes::from_static(b"body")
        ));
        let mut headers = headers;
        headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        assert!(!share_connections(&Method::GET, &uri, &headers, &body));
    }

    #[test]
    fn clash_target_url_preserves_path_and_query() {
        let uri = "/api/clash/proxies/node%201/delay?timeout=3000&url=http://example.com"
            .parse()
            .unwrap();

        assert_eq!(
            clash_target_url(&uri),
            format!(
                "{}/proxies/node%201/delay?timeout=3000&url=http://example.com",
                crate::services::singbox::CLASH_API_BASE
            )
        );
    }
}
