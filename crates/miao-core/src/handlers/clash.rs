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

use crate::state::AppState;

const CLASH_API_BASE: &str = "http://127.0.0.1:6262";
const CLASH_TRAFFIC_WS: &str = "ws://127.0.0.1:6262/traffic";

fn clash_target_url(uri: &axum::http::Uri) -> String {
    let path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let suffix = path_and_query
        .strip_prefix("/api/clash")
        .unwrap_or(path_and_query);
    let suffix = if suffix.is_empty() { "/" } else { suffix };
    format!("{CLASH_API_BASE}{suffix}")
}

pub async fn proxy_clash_http(
    State(state): State<Arc<AppState>>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let target = clash_target_url(&uri);
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

    match request.send().await {
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
    }
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
    use super::clash_target_url;

    #[test]
    fn clash_target_url_preserves_path_and_query() {
        let uri = "/api/clash/proxies/node%201/delay?timeout=3000&url=http://example.com"
            .parse()
            .unwrap();

        assert_eq!(
            clash_target_url(&uri),
            "http://127.0.0.1:6262/proxies/node%201/delay?timeout=3000&url=http://example.com"
        );
    }
}
