use super::*;

pub(super) struct ReadyProbe;

impl DataPlaneProbe for ReadyProbe {
    fn tun_ready(&self) -> bool {
        true
    }
    async fn clash_ready(&mut self) -> bool {
        true
    }
}

#[tokio::test]
async fn http_probe_requires_success_and_bounds_a_stalled_response() {
    use axum::{http::StatusCode, routing::get, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/ok", get(|| async { StatusCode::OK }))
        .route(
            "/unready",
            get(|| async { StatusCode::SERVICE_UNAVAILABLE }),
        )
        .route(
            "/stalled",
            get(|| async {
                sleep(Duration::from_secs(2)).await;
                StatusCode::OK
            }),
        );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for (path, expected) in [("/ok", true), ("/unready", false), ("/stalled", false)] {
        let mut probe = SystemProbe {
            client: &client,
            clash_url: format!("{base}{path}"),
        };
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), probe.clash_ready())
                .await
                .unwrap(),
            expected
        );
    }
    server.abort();
    let _ = server.await;
    let mut probe = SystemProbe {
        client: &client,
        clash_url: format!("{base}/ok"),
    };
    assert!(
        !probe.clash_ready().await,
        "a refused connection is not ready"
    );
}

#[cfg(unix)]
mod controller {
    use super::*;
    use std::collections::VecDeque;

    async fn fixture() -> (tempfile::TempDir, Arc<AppState>, u64, u32) {
        let (root, state) = crate::test_support::isolated_stopped_state(Default::default());
        state.lifecycle.request_running(true);
        let generation = state.lifecycle.begin(KernelOperation::Start);
        let child = tokio::process::Command::new("sleep")
            .arg("60")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        *state.sing_process.lock().await = Some(SingBoxProcess {
            child,
            started_at: std::time::Instant::now(),
        });
        (root, state, generation, pid)
    }

    async fn reap(state: &AppState) {
        if let Some(mut process) = state.sing_process.lock().await.take() {
            process.child.kill().await.unwrap();
        }
    }

    struct ScriptedProbe {
        tun: bool,
        responses: VecDeque<bool>,
        fallback: bool,
        started: Instant,
        observations: Vec<Duration>,
    }

    impl ScriptedProbe {
        fn new(tun: bool, fallback: bool, responses: &[bool]) -> Self {
            Self {
                tun,
                fallback,
                responses: responses.iter().copied().collect(),
                started: Instant::now(),
                observations: Vec::new(),
            }
        }
    }

    impl DataPlaneProbe for ScriptedProbe {
        fn tun_ready(&self) -> bool {
            self.tun
        }
        async fn clash_ready(&mut self) -> bool {
            self.observations.push(self.started.elapsed());
            self.responses.pop_front().unwrap_or(self.fallback)
        }
    }

    #[tokio::test(start_paused = true)]
    async fn startup_waits_for_settle_and_two_successful_observations() {
        let (_root, state, generation, _) = fixture().await;
        let mut probe = ScriptedProbe::new(true, true, &[]);
        wait_with_probe(&state, generation, Operation::Start, &mut probe)
            .await
            .unwrap();
        assert_eq!(
            probe.observations,
            [Duration::from_millis(100), Duration::from_millis(125)]
        );
        assert!(
            !state.lifecycle.snapshot().ready,
            "publication still belongs to the controller"
        );
        reap(&state).await;
    }

    #[tokio::test(start_paused = true)]
    async fn reload_uses_its_longer_settle_time() {
        let (_root, state, _, pid) = fixture().await;
        let generation = state.lifecycle.begin(KernelOperation::Reload);
        let mut probe = ScriptedProbe::new(true, true, &[]);
        wait_with_probe(&state, generation, Operation::Reload { pid }, &mut probe)
            .await
            .unwrap();
        assert_eq!(
            probe.observations,
            [Duration::from_millis(500), Duration::from_millis(525)]
        );
        reap(&state).await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_observation_resets_the_success_streak() {
        let (_root, state, generation, _) = fixture().await;
        let mut probe = ScriptedProbe::new(true, true, &[true, false, true, true]);
        wait_with_probe(&state, generation, Operation::Start, &mut probe)
            .await
            .unwrap();
        assert_eq!(probe.observations.len(), 4);
        assert_eq!(probe.started.elapsed(), Duration::from_millis(175));
        reap(&state).await;
    }

    #[tokio::test(start_paused = true)]
    async fn both_signals_are_required_and_only_start_timeout_terminates_the_child() {
        for (tun, clash) in [(false, true), (true, false)] {
            for reload in [false, true] {
                let (_root, state, generation, pid) = fixture().await;
                let operation = if reload {
                    Operation::Reload { pid }
                } else {
                    Operation::Start
                };
                let mut probe = ScriptedProbe::new(tun, clash, &[]);
                let error = wait_with_probe(&state, generation, operation, &mut probe)
                    .await
                    .unwrap_err();
                assert_eq!(error.to_string(), operation.timeout());
                assert!(probe.started.elapsed() >= Duration::from_secs(8));
                assert_eq!(state.sing_process.lock().await.is_some(), reload);
                assert!(!state.lifecycle.snapshot().ready);
                reap(&state).await;
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn changed_owner_or_pid_cannot_pass_readiness() {
        let (_root, state, old_generation, pid) = fixture().await;
        let generation = state.lifecycle.begin(KernelOperation::Reload);
        let mut probe = ScriptedProbe::new(true, true, &[]);
        assert!(
            wait_with_probe(&state, old_generation, Operation::Start, &mut probe)
                .await
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        assert!(wait_with_probe(
            &state,
            generation,
            Operation::Reload { pid: pid + 1 },
            &mut probe
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("process changed"));
        assert!(probe.observations.is_empty());
        assert_eq!(
            state.sing_process.lock().await.as_ref().unwrap().child.id(),
            Some(pid)
        );
        reap(&state).await;
    }

    #[tokio::test(start_paused = true)]
    async fn an_exited_child_fails_before_probing_the_data_plane() {
        let (_root, state, generation, _) = fixture().await;
        state
            .sing_process
            .lock()
            .await
            .as_mut()
            .unwrap()
            .child
            .kill()
            .await
            .unwrap();
        let mut probe = ScriptedProbe::new(true, true, &[]);
        assert!(
            wait_with_probe(&state, generation, Operation::Start, &mut probe)
                .await
                .unwrap_err()
                .to_string()
                .contains("exited during startup")
        );
        assert!(probe.observations.is_empty());
        assert!(state.sing_process.lock().await.is_none());
    }

    struct SupersedingProbe {
        state: Arc<AppState>,
    }

    impl DataPlaneProbe for SupersedingProbe {
        fn tun_ready(&self) -> bool {
            true
        }
        async fn clash_ready(&mut self) -> bool {
            let generation = self.state.lifecycle.begin(KernelOperation::Start);
            self.state.lifecycle.finish(generation, RuntimePhase::Ready);
            sleep(READY_TIMEOUT).await;
            false
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_late_probe_timeout_cannot_kill_a_new_owner() {
        let (_root, state, generation, pid) = fixture().await;
        let mut probe = SupersedingProbe {
            state: state.clone(),
        };
        assert!(
            wait_with_probe(&state, generation, Operation::Start, &mut probe)
                .await
                .is_err()
        );
        assert_eq!(
            state.sing_process.lock().await.as_ref().unwrap().child.id(),
            Some(pid)
        );
        assert!(state.lifecycle.snapshot().ready);
        reap(&state).await;
    }
}
