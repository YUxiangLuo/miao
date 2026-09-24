//! Phase-one gateway transport contract, using actual reqwest and sing-box.
//! Run through scripts/test-ai-transport.sh, never against the host proxy.
#![cfg(target_os = "linux")]

use std::{
    convert::Infallible,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{
    body::Body,
    extract::{connect_info::Connected, ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::{net::TcpListener, process::Child, time::timeout};
use tokio_rustls::{rustls, TlsAcceptor};

struct TlsListener(TcpListener, TlsAcceptor);

impl axum::serve::Listener for TlsListener {
    type Io = tokio_rustls::server::TlsStream<tokio::net::TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (socket, addr) = self.0.accept().await.unwrap();
            if let Ok(Ok(tls)) = timeout(Duration::from_secs(2), self.1.accept(socket)).await {
                return (tls, addr);
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.0.local_addr()
    }
}

#[derive(Clone)]
struct Peer(SocketAddr);

impl Connected<axum::serve::IncomingStream<'_, TlsListener>> for Peer {
    fn connect_info(stream: axum::serve::IncomingStream<'_, TlsListener>) -> Self {
        Self(*stream.remote_addr())
    }
}

#[derive(Clone, Default)]
pub(crate) struct Observations {
    pub(crate) requests: Arc<AtomicUsize>,
    pub(crate) streams: Arc<AtomicUsize>,
}

struct ActiveStream(Arc<AtomicUsize>);
impl Drop for ActiveStream {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn probe(
    State(seen): State<Observations>,
    ConnectInfo(Peer(peer)): ConnectInfo<Peer>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    seen.requests.fetch_add(1, Ordering::SeqCst);
    Json(json!({
        "peer": peer.to_string(), "body": body,
        "authorization": headers.get("authorization").and_then(|h| h.to_str().ok()),
        "proxy_authorization": headers.get("proxy-authorization").and_then(|h| h.to_str().ok()),
        "cookie": headers.get("cookie").and_then(|h| h.to_str().ok()),
    }))
}

async fn events(
    State(seen): State<Observations>,
    ConnectInfo(Peer(peer)): ConnectInfo<Peer>,
) -> Response {
    seen.streams.fetch_add(1, Ordering::SeqCst);
    let stream = futures::stream::unfold(
        (ActiveStream(seen.streams), 0),
        move |(guard, n)| async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            let event = format!("data: {}\n\n", json!({"peer": peer.to_string(), "n": n}));
            Some((Ok::<_, Infallible>(event), (guard, n + 1)))
        },
    );
    (
        [("content-type", "text/event-stream")],
        Body::from_stream(stream),
    )
        .into_response()
}

async fn gateway_probe(
    state: State<Observations>,
    peer: ConnectInfo<Peer>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Response {
    if body.0["stream"] == true {
        return events(state, peer).await;
    }
    if body.0["model"] == "redirect" {
        return (StatusCode::TEMPORARY_REDIRECT, [("location", "/probe")]).into_response();
    }
    probe(state, peer, headers, body).await.into_response()
}

fn openssl(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("openssl")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "openssl: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) async fn provider(
    dir: &Path,
    seen: Observations,
) -> (String, reqwest::Certificate, tokio::task::JoinHandle<()>) {
    // A fresh test-only key, never a real provider key and never committed.
    openssl(
        dir,
        &[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "1",
            "-subj",
            "/CN=localhost",
            "-addext",
            "subjectAltName=DNS:localhost,IP:127.0.0.1",
            "-addext",
            "basicConstraints=critical,CA:FALSE",
            "-keyout",
            "key.pem",
            "-out",
            "cert.pem",
        ],
    );
    openssl(
        dir,
        &[
            "x509", "-in", "cert.pem", "-outform", "DER", "-out", "cert.der",
        ],
    );
    openssl(
        dir,
        &[
            "pkcs8", "-topk8", "-nocrypt", "-in", "key.pem", "-outform", "DER", "-out", "key.der",
        ],
    );
    let cert = std::fs::read(dir.join("cert.der")).unwrap();
    let trust = reqwest::Certificate::from_der(&cert).unwrap();
    let key =
        rustls::pki_types::PrivatePkcs8KeyDer::from(std::fs::read(dir.join("key.der")).unwrap());
    let tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert.into()], key.into())
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Router::new()
        .route("/probe", post(probe))
        .route("/events", get(events))
        .route("/v1/chat/completions", post(gateway_probe))
        .route("/v1/responses", post(gateway_probe))
        .route(
            "/redirect",
            get(|| async { (StatusCode::TEMPORARY_REDIRECT, [("location", "/events")]) }),
        )
        .with_state(seen);
    let server = tokio::spawn(async move {
        axum::serve(
            TlsListener(listener, TlsAcceptor::from(Arc::new(tls))),
            router.into_make_service_with_connect_info::<Peer>(),
        )
        .await
        .unwrap();
    });
    (format!("https://localhost:{}", addr.port()), trust, server)
}

pub(crate) fn reserve_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

pub(crate) async fn kernel(
    binary: &Path,
    dir: &Path,
    name: &str,
    config: Value,
    port: u16,
) -> Child {
    let path = dir.join(format!("{name}.json"));
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let log_path = dir.join(format!("{name}.log"));
    let log = std::fs::File::create(&log_path).unwrap();
    let mut child = tokio::process::Command::new(binary)
        .args(["run", "-c"])
        .arg(path)
        .current_dir(dir)
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let ready = timeout(Duration::from_secs(10), async {
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                panic!(
                    "{name} exited {status}: {}",
                    std::fs::read_to_string(&log_path).unwrap()
                );
            }
            if tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(
        ready.is_ok(),
        "{name} not ready: {}",
        std::fs::read_to_string(log_path).unwrap()
    );
    child
}

// This client policy is the contract for the future gateway transport: no
// environment proxy, no redirects, verified TLS, and no total stream timeout.
fn client(port: u16, cert: &reqwest::Certificate, password: &str) -> Client {
    Client::builder()
        .no_proxy()
        .proxy(
            reqwest::Proxy::all(format!("http://127.0.0.1:{port}"))
                .unwrap()
                .basic_auth("gateway", password),
        )
        .add_root_certificate(cert.clone())
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(2))
        .http1_only()
        .build()
        .unwrap()
}

async fn observe(client: &Client, base: &str) -> Value {
    let body = json!({"model":"fixture/model", "stream":false, "messages":[{"role":"user","content":"synthetic"}], "tools":[{"type":"function","function":{"name":"fixture"}}], "provider_extension":{"keep":true}});
    let response: Value = timeout(Duration::from_secs(5), async {
        client
            .post(format!("{base}/probe"))
            .bearer_auth("synthetic-upstream-key")
            .json(&body)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    })
    .await
    .unwrap();
    assert_eq!(response["body"], body);
    assert_eq!(response["authorization"], "Bearer synthetic-upstream-key");
    assert!(
        response["proxy_authorization"].is_null(),
        "internal proxy credential leaked through TLS"
    );
    response
}

pub(crate) fn assert_exit(response: &Value, ip: &str) {
    let peer: SocketAddr = response["peer"].as_str().unwrap().parse().unwrap();
    assert_eq!(peer.ip().to_string(), ip);
}

pub(crate) struct Events {
    pub(crate) response: reqwest::Response,
    pub(crate) pending: Vec<u8>,
    pub(crate) expected: u64,
}

impl Events {
    pub(crate) async fn next(&mut self) -> Value {
        timeout(Duration::from_secs(3), async {
            loop {
                // A TCP/TLS chunk is not an SSE frame. Preserve partial and
                // coalesced frames, including when the consumer pauses.
                if let Some(end) = self.pending.windows(2).position(|pair| pair == b"\n\n") {
                    let frame: Vec<_> = self.pending.drain(..end + 2).collect();
                    let text = std::str::from_utf8(&frame).unwrap();
                    let value: Value =
                        serde_json::from_str(text.trim().strip_prefix("data: ").unwrap()).unwrap();
                    assert_eq!(value["n"], self.expected, "SSE event lost or replayed");
                    self.expected += 1;
                    return value;
                }
                let bytes = self.response.chunk().await.unwrap().expect("stream ended");
                self.pending.extend_from_slice(&bytes);
                assert!(self.pending.len() < 64 * 1024, "oversized fixture frame");
            }
        })
        .await
        .unwrap()
    }
}

#[tokio::test]
#[ignore = "real kernels: run scripts/test-ai-transport.sh in an isolated Linux network namespace"]
async fn ai_transport_contract() {
    let host = std::env::var("MIAO_TEST_HOST_NETNS").expect("use scripts/test-ai-transport.sh");
    assert_ne!(
        std::fs::read_link("/proc/self/ns/net").unwrap(),
        PathBuf::from(host)
    );
    let _ = rustls::crypto::ring::default_provider().install_default();
    let dir = tempfile::tempdir().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let seen = Observations::default();
    let (base, cert, server) = provider(dir.path(), seen.clone()).await;
    let a = reserve_port();
    let b = reserve_port();
    let inbound = reserve_port();
    let control = reserve_port();
    let dead = reserve_port();
    let mut upstream = kernel(&root.join("embedded/sing-box-host"), dir.path(), "upstream", json!({
        "log":{"level":"error"},
        "dns":{"servers":[{"type":"local","tag":"local"}]},
        "inbounds":[
            {"type":"shadowsocks","tag":"a","listen":"127.0.0.1","listen_port":a,"method":"aes-128-gcm","password":"synthetic-a"},
            {"type":"shadowsocks","tag":"b","listen":"127.0.0.1","listen_port":b,"method":"aes-128-gcm","password":"synthetic-b"}],
        "outbounds":[{"type":"direct","tag":"a","inet4_bind_address":"127.0.0.2"},{"type":"direct","tag":"b","inet4_bind_address":"127.0.0.3"}],
        "route":{"default_domain_resolver":"local","rules":[{"inbound":["a"],"outbound":"a"},{"inbound":["b"],"outbound":"b"}]}
    }), a).await;
    let proxy_config = json!({
        "log":{"level":"error"},
        "experimental":{"clash_api":{"external_controller":format!("127.0.0.1:{control}"),"secret":"synthetic-control"}},
        "inbounds":[{"type":"http","tag":"ai-in","listen":"127.0.0.1","listen_port":inbound,"users":[{"username":"gateway","password":"synthetic-internal"}]}],
        "outbounds":[
            {"type":"selector","tag":"ai","outbounds":["a","b","dead"],"default":"a","interrupt_exist_connections":false},
            {"type":"shadowsocks","tag":"a","server":"127.0.0.1","server_port":a,"method":"aes-128-gcm","password":"synthetic-a"},
            {"type":"shadowsocks","tag":"b","server":"127.0.0.1","server_port":b,"method":"aes-128-gcm","password":"synthetic-b"},
            {"type":"shadowsocks","tag":"dead","server":"127.0.0.1","server_port":dead,"method":"aes-128-gcm","password":"synthetic-dead"}],
        "route":{"rules":[{"inbound":["ai-in"],"action":"route","outbound":"ai"},{"action":"reject"}]}
    });
    let mut proxy = kernel(
        &root.join("embedded/sing-box-amd64"),
        dir.path(),
        "client",
        proxy_config.clone(),
        control,
    )
    .await;
    let control_client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let select = |name: &'static str| {
        control_client
            .put(format!("http://127.0.0.1:{control}/proxies/ai"))
            .bearer_auth("synthetic-control")
            .json(&json!({"name":name}))
    };

    let old = client(inbound, &cert, "synthetic-internal");
    let first = observe(&old, &base).await;
    assert_exit(&first, "127.0.0.2");
    let response = old
        .get(format!("{base}/events"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let mut stream = Events {
        response,
        pending: Vec::new(),
        expected: 0,
    };
    assert_exit(&stream.next().await, "127.0.0.2");
    let connections: Value = control_client
        .get(format!("http://127.0.0.1:{control}/connections"))
        .bearer_auth("synthetic-control")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        connections["connections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|connection| {
                connection["metadata"]["host"] == "localhost"
                    && connection["chains"]
                        .as_array()
                        .unwrap()
                        .contains(&json!("ai"))
            }),
        "CONNECT must preserve the domain and traverse the AI selector"
    );
    // Keep a second, idle HTTP/1 connection alongside the active SSE tunnel.
    assert_exit(&observe(&old, &base).await, "127.0.0.2");
    assert_eq!(
        select("b").send().await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
    assert_exit(&stream.next().await, "127.0.0.2");

    // Negative control: retaining the old pool really does retain the old
    // exit. Merely updating the selector cannot satisfy the gateway contract.
    let reused = observe(&old, &base).await;
    assert_exit(&reused, "127.0.0.2");
    let new = client(inbound, &cert, "synthetic-internal");
    assert_exit(&observe(&new, &base).await, "127.0.0.3");
    drop(old);
    // Exceed AppState's existing 30-second total timeout; that client cannot
    // be reused by a long-lived gateway request.
    let until = tokio::time::Instant::now() + Duration::from_secs(31);
    while tokio::time::Instant::now() < until {
        assert_exit(&stream.next().await, "127.0.0.2");
    }
    drop(stream);
    timeout(Duration::from_secs(3), async {
        while seen.streams.load(Ordering::SeqCst) != 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("dropping the response must cancel the upstream stream");

    assert_eq!(
        new.get(format!("{base}/redirect"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::TEMPORARY_REDIRECT
    );
    let count = seen.requests.load(Ordering::SeqCst);
    let denied = client(inbound, &cert, "wrong");
    assert!(denied
        .post(format!("{base}/probe"))
        .json(&json!({}))
        .send()
        .await
        .is_err());
    let untrusted = Client::builder()
        .no_proxy()
        .proxy(
            reqwest::Proxy::all(format!("http://127.0.0.1:{inbound}"))
                .unwrap()
                .basic_auth("gateway", "synthetic-internal"),
        )
        .connect_timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let error = untrusted
        .post(format!("{base}/probe"))
        .json(&json!({}))
        .send()
        .await
        .unwrap_err();
    assert!(
        error.is_connect() && format!("{error:?}").contains("UnknownIssuer"),
        "expected untrusted fixture certificate, got {error:?}"
    );
    assert_eq!(seen.requests.load(Ordering::SeqCst), count);
    assert_eq!(
        select("missing-node").send().await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_exit(
        &observe(&client(inbound, &cert, "synthetic-internal"), &base).await,
        "127.0.0.3",
    );
    assert_eq!(
        select("dead").send().await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
    let count = seen.requests.load(Ordering::SeqCst);
    let failed = client(inbound, &cert, "synthetic-internal");
    assert!(timeout(
        Duration::from_secs(5),
        failed.post(format!("{base}/probe")).json(&json!({})).send()
    )
    .await
    .unwrap()
    .is_err());
    assert_eq!(
        seen.requests.load(Ordering::SeqCst),
        count,
        "unavailable exit must not fall back"
    );

    assert_eq!(
        select("b").send().await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
    let active = client(inbound, &cert, "synthetic-internal");
    let response = active
        .get(format!("{base}/events"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let mut interrupted = Events {
        response,
        pending: Vec::new(),
        expected: 0,
    };
    assert_exit(&interrupted.next().await, "127.0.0.3");
    proxy.kill().await.unwrap();
    timeout(Duration::from_secs(3), async {
        while let Ok(Some(_)) = interrupted.response.chunk().await {}
    })
    .await
    .expect("kernel death must terminate the stream, not silently replay it");
    let stopped = client(inbound, &cert, "synthetic-internal");
    assert!(stopped
        .post(format!("{base}/probe"))
        .json(&json!({}))
        .send()
        .await
        .is_err());
    assert_eq!(seen.requests.load(Ordering::SeqCst), count);
    proxy = kernel(
        &root.join("embedded/sing-box-amd64"),
        dir.path(),
        "restarted",
        proxy_config,
        control,
    )
    .await;
    assert_exit(
        &observe(&client(inbound, &cert, "synthetic-internal"), &base).await,
        "127.0.0.2",
    );

    proxy.kill().await.unwrap();
    upstream.kill().await.unwrap();
    server.abort();
    let _ = server.await;
    println!("PASS: authenticated CONNECT, verified TLS, two Shadowsocks exits, pool replacement, old-stream survival, cancellation, no redirects, failed-exit refusal, kernel stop/restart");
}
