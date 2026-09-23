use std::{sync::Mutex, time::Duration};

use axum::{
    body::Bytes,
    http::{header, HeaderValue, StatusCode},
};
use futures::{
    future::{BoxFuture, WeakShared},
    FutureExt,
};

#[derive(Clone)]
pub struct ConnectionResponse {
    pub status: StatusCode,
    pub content_type: Option<HeaderValue>,
    pub body: Bytes,
}

type Flight = WeakShared<BoxFuture<'static, Result<ConnectionResponse, String>>>;

/// Share only overlapping reads. Retaining completed snapshots would distort
/// the UI's counter-derived speeds. A weak handle also cancels upstream I/O
/// when the last waiter disappears; one waiter's timeout cannot cancel others.
#[derive(Default)]
pub struct ConnectionRequests {
    flight: Mutex<Option<(u64, String, Flight)>>,
}

impl ConnectionRequests {
    /// A successful mutation must not let later reads join a pre-mutation read.
    pub fn invalidate(&self) {
        self.flight.lock().unwrap().take();
    }

    pub async fn get(
        &self,
        client: &reqwest::Client,
        url: &str,
        generation: u64,
    ) -> Result<ConnectionResponse, String> {
        let request = {
            let mut slot = self.flight.lock().unwrap();
            let existing = slot.as_ref().and_then(|(epoch, target, flight)| {
                (*epoch == generation && target == url)
                    .then(|| flight.upgrade())
                    .flatten()
                    .filter(|flight| flight.peek().is_none())
            });
            if let Some(existing) = existing {
                existing
            } else {
                let client = client.clone();
                let target = url.to_owned();
                let request = async move {
                    let response = client
                        .get(target)
                        .timeout(Duration::from_secs(10))
                        .send()
                        .await
                        .map_err(|e| e.to_string())?;
                    let status = response.status();
                    let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
                    let body = response.bytes().await.map_err(|e| e.to_string())?;
                    Ok(ConnectionResponse {
                        status,
                        content_type,
                        body,
                    })
                }
                .boxed()
                .shared();
                *slot = Some((generation, url.to_owned(), request.downgrade().unwrap()));
                request
            }
        };
        request.await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::sync::{mpsc, Semaphore};

    struct Server {
        url: String,
        gate: Arc<Semaphore>,
        hits: mpsc::UnboundedReceiver<usize>,
        count: Arc<AtomicUsize>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for Server {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    impl Server {
        async fn new(status: StatusCode) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/connections", listener.local_addr().unwrap());
            let gate = Arc::new(Semaphore::new(0));
            let count = Arc::new(AtomicUsize::new(0));
            let (tx, hits) = mpsc::unbounded_channel();
            let app = axum::Router::new().fallback(axum::routing::get({
                let gate = gate.clone();
                let count = count.clone();
                move || {
                    let (gate, count, tx) = (gate.clone(), count.clone(), tx.clone());
                    async move {
                        let n = count.fetch_add(1, Ordering::SeqCst) + 1;
                        let _ = tx.send(n);
                        gate.acquire().await.unwrap().forget();
                        (
                            status,
                            [(header::CONTENT_TYPE, "application/x-clash-test")],
                            vec![0xff, 0, n as u8],
                        )
                    }
                }
            }));
            let task = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            Self {
                url,
                gate,
                hits,
                count,
                task,
            }
        }

        async fn hit(&mut self) -> usize {
            tokio::time::timeout(Duration::from_secs(5), self.hits.recv())
                .await
                .expect("request did not reach isolated upstream")
                .unwrap()
        }
    }

    #[tokio::test]
    async fn burst_is_one_request_and_completed_results_are_never_cached() {
        for status in [StatusCode::OK, StatusCode::SERVICE_UNAVAILABLE] {
            let mut server = Server::new(status).await;
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let requests = ConnectionRequests::default();
            let url = server.url.clone();
            let mut burst = Box::pin(futures::future::join_all(
                (0..32).map(|_| requests.get(&client, &url, 7)),
            ));
            tokio::select! {
                _ = &mut burst => panic!("response before release"),
                hit = server.hit() => assert_eq!(hit, 1),
            }
            server.gate.add_permits(32);
            for result in burst.await {
                let response = result.unwrap();
                assert_eq!(response.status, status);
                assert_eq!(response.content_type.unwrap(), "application/x-clash-test");
                assert_eq!(response.body.as_ref(), &[0xff, 0, 1]);
            }
            let next = requests.get(&client, &url, 7).await.unwrap();
            assert_eq!(next.body.as_ref(), &[0xff, 0, 2]);
            assert_eq!(server.count.load(Ordering::SeqCst), 2);
        }
    }

    #[tokio::test]
    async fn generations_do_not_share_in_flight_results() {
        let mut server = Server::new(StatusCode::OK).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let requests = ConnectionRequests::default();
        let url = server.url.clone();
        let mut both = Box::pin(futures::future::join(
            requests.get(&client, &url, 7),
            requests.get(&client, &url, 8),
        ));
        for expected in [1, 2] {
            tokio::select! {
                _ = &mut both => panic!("response before release"),
                hit = server.hit() => assert_eq!(hit, expected),
            }
        }
        server.gate.add_permits(2);
        let (a, b) = both.await;
        assert_ne!(a.unwrap().body, b.unwrap().body);
        assert_eq!(server.count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn invalidation_detaches_old_flight_without_cancelling_its_waiters() {
        let mut server = Server::new(StatusCode::OK).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let requests = ConnectionRequests::default();
        let url = server.url.clone();
        let mut old = Box::pin(requests.get(&client, &url, 7));
        tokio::select! {
            _ = &mut old => panic!("response before release"),
            hit = server.hit() => assert_eq!(hit, 1),
        }
        requests.invalidate();
        server.gate.add_permits(2);
        let (old, new) = tokio::join!(old, requests.get(&client, &url, 7));
        assert_eq!(old.unwrap().body.as_ref(), &[0xff, 0, 1]);
        assert_eq!(new.unwrap().body.as_ref(), &[0xff, 0, 2]);
    }

    #[tokio::test]
    async fn truncated_body_is_an_error_and_next_request_retries() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/connections", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for length in [8, 1] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = [0; 2048];
                assert!(socket.read(&mut buf).await.unwrap() > 0);
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
                socket.write_all(&[0xfe]).await.unwrap();
                socket.shutdown().await.unwrap();
            }
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let requests = ConnectionRequests::default();
        assert!(requests.get(&client, &url, 7).await.is_err());
        let response = requests.get(&client, &url, 7).await.unwrap();
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.body.as_ref(), &[0xfe]);
        assert!(response.content_type.is_none());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn timeout_does_not_cancel_other_waiter_and_last_cancellation_releases_flight() {
        let mut server = Server::new(StatusCode::OK).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let requests = ConnectionRequests::default();
        let url = server.url.clone();
        let mut survivor = Box::pin(requests.get(&client, &url, 7));
        tokio::select! {
            _ = &mut survivor => panic!("response before release"),
            hit = server.hit() => assert_eq!(hit, 1),
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(20), requests.get(&client, &url, 7))
                .await
                .is_err()
        );
        server.gate.add_permits(1);
        assert_eq!(survivor.await.unwrap().body.as_ref(), &[0xff, 0, 1]);
        assert_eq!(server.count.load(Ordering::SeqCst), 1);

        let mut cancelled = Box::pin(requests.get(&client, &url, 7));
        tokio::select! {
            _ = &mut cancelled => panic!("response before release"),
            hit = server.hit() => assert_eq!(hit, 2),
        }
        drop(cancelled);
        assert!(requests
            .flight
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .2
            .upgrade()
            .is_none());
        server.gate.add_permits(2);
        assert_eq!(
            requests.get(&client, &url, 7).await.unwrap().body.as_ref(),
            &[0xff, 0, 3]
        );
    }
}
