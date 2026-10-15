//! Manual microbenchmark of the status projection, excluding HTTP and kernel
//! queries. Run in release mode with `--ignored --nocapture` (see DEV_NOTES.md).
use super::{runtime_config_status, RuntimeConfigStatus};
use crate::{models::Config, state::AppState, test_support::isolated_stopped_state};
use std::{hint::black_box, time::Instant};

// Retain the former full-clone implementation as the measurement baseline.
async fn full_clone_status(state: &AppState) -> RuntimeConfigStatus {
    let config = black_box(state.config.read().await.clone());
    let requested_node_select = *state.node_select_preference.read().await;
    let mut multiplier_options = state.available_multipliers.read().await.clone();
    if let Some(current) = config.max_multiplier {
        if !multiplier_options.contains(&current) {
            multiplier_options.push(current);
            multiplier_options.sort_unstable();
        }
    }
    RuntimeConfigStatus {
        route_mode: config.route_mode,
        node_select: config.node_select,
        max_multiplier: config.max_multiplier,
        mcp: config.mcp,
        manual_node_count: config.nodes.len(),
        requested_node_select,
        multiplier_options,
    }
}

#[tokio::test]
#[ignore = "manual release benchmark; timing is diagnostic, not a CI assertion"]
async fn benchmark_status_projection() {
    const ITERATIONS: usize = 500;
    for count in [0, 100, 10_000] {
        let config = Config {
            nodes: (0..count).map(|n| format!(
                r#"{{"type":"trojan","tag":"node-{n}","server":"node-{n}.example.com","server_port":443,"password":"benchmark-password","tls":{{"enabled":true}}}}"#
            )).collect(),
            custom_rules: (0..count).map(|n| format!(
                r#"{{"domain_suffix":"domain-{n}.example","outbound":"proxy"}}"#
            )).collect(),
            ..Config::default()
        };
        let (_root, state) = isolated_stopped_state(config);
        assert_eq!(
            full_clone_status(&state).await,
            runtime_config_status(&state).await
        );
        for _ in 0..20 {
            black_box(full_clone_status(&state).await);
            black_box(runtime_config_status(&state).await);
        }
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            black_box(full_clone_status(black_box(&state)).await);
        }
        let before = start.elapsed().as_secs_f64() * 1_000_000.0 / ITERATIONS as f64;
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            black_box(runtime_config_status(black_box(&state)).await);
        }
        let after = start.elapsed().as_secs_f64() * 1_000_000.0 / ITERATIONS as f64;
        println!("nodes={count}, rules={count}: full clone {before:.3} us/call; projection {after:.3} us/call");
    }
}
