use super::*;
use axum::http::StatusCode;

#[tokio::test]
async fn healthy_kernel_confirms_upgrade_before_subscription_recovery_finishes() {
    // Keep markers away from the actual executable and from concurrent tests.
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("upgrade-health-test");
    std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(8),
        tokio::process::Command::new(&executable)
            .args([
                "--exact",
                "runtime::startup::recovery_tests::upgrade_health::upgrade_health_child",
                "--nocapture",
            ])
            .env("MIAO_TEST_UPGRADE_HEALTH_CHILD", "1")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn upgrade_health_child() {
    if std::env::var("MIAO_TEST_UPGRADE_HEALTH_CHILD").as_deref() != Ok("1") {
        return;
    }
    let executable = std::env::current_exe().unwrap();
    let pending =
        std::path::PathBuf::from(format!("{}.upgrade-pending.json", executable.display()));
    let backup = std::path::PathBuf::from(format!("{}.bak", executable.display()));
    std::fs::write(
        &pending,
        serde_json::to_vec(&serde_json::json!({
            "expected_version": crate::VERSION, "state":"booting", "boot_pid":std::process::id()
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(&backup, "isolated fake previous executable").unwrap();
    let app = Router::new().route("/sub", get(|| async { StatusCode::SERVICE_UNAVAILABLE }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cfg = config(format!("http://{}/sub", listener.local_addr().unwrap()));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_root, state) = fixture(cfg.clone(), true);
    let initializing = state.clone();
    let task = tokio::spawn(async move { initialize_runtime(cfg, initializing, false).await });
    tokio::time::timeout(Duration::from_secs(4), async {
        while !state.lifecycle.snapshot().ready {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let confirmed = tokio::time::timeout(Duration::from_millis(250), async {
        while pending.exists() || backup.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok();
    let still_retrying = !task.is_finished();
    state.lifecycle.shutdown();
    state.next_sub_refresh();
    stop_sing_internal(&state).await;
    task.abort();
    let _ = task.await;
    server.abort();
    assert!(still_retrying, "subscription failure must still be retried");
    assert!(
        confirmed,
        "healthy kernel recovery must confirm the upgrade before waiting for subscription recovery"
    );
}
