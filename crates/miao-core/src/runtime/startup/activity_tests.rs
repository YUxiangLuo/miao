use super::*;

#[tokio::test]
async fn initialization_after_shutdown_preserves_stopped_status() {
    let config = Config {
        subs: vec!["http://127.0.0.1:1/must-not-fetch".to_string()],
        ..Default::default()
    };
    let (_root, state) = crate::test_support::isolated_stopped_state(config.clone());
    state.initializing.store(true, Ordering::Relaxed);
    state.lifecycle.shutdown();
    stop_sing_internal(&state).await;
    initialize_runtime(config, state.clone(), false).await;
    let status = crate::services::commands::service::get_status(state)
        .await
        .data
        .unwrap();
    assert_eq!(status.phase, RuntimePhase::Stopped);
    assert!(!status.running);
    assert!(!status.ready);
    assert!(!status.initializing);
}

#[tokio::test]
async fn unavailable_data_plane_defers_to_an_in_flight_foreground_operation() {
    let config = Config {
        subs: vec!["http://127.0.0.1:1/must-not-fetch".to_string()],
        ..Default::default()
    };
    let state = crate::test_support::app_state(config);
    let generation = state.next_sub_refresh();
    let _foreground = state.subscription_refresh.foreground(generation);
    assert_eq!(
        recover_data_plane_once(&state).await,
        RecoveryOutcome::RetryDataPlane
    );
    assert!(
        state.sub_status.lock().await.is_empty(),
        "recovery must not start competing HTTP requests"
    );
}

#[tokio::test]
async fn elapsed_retry_waits_for_foreground_commit_not_just_http_completion() {
    let config = Config::default();
    let state = crate::test_support::app_state(config.clone());
    let generation = state.next_sub_refresh();
    let foreground = state.subscription_refresh.foreground(generation);
    let request = state.subscription_refresh.begin(generation);
    let waiting_state = state.clone();
    let waiter = tokio::spawn(async move {
        wait_for_background_retry(&config, &waiting_state, generation, Duration::ZERO).await
    });
    request.finish(crate::models::SubscriptionFetchReport {
        successful_sources: 1,
        ..Default::default()
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(
        !waiter.is_finished(),
        "HTTP completion is not the transaction commit"
    );
    state
        .sub_refresh_success_generation
        .store(generation, Ordering::Relaxed);
    drop(foreground);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap(),
        None
    );
}
