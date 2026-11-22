use super::restore_runtime_files;
use crate::{models::Config, test_support::isolated_stopped_state};

#[tokio::test]
async fn runtime_file_rollback_attempts_both_files_and_reports_both_failures() {
    for (fail_runtime, fail_bindings) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let (_root, state) = isolated_stopped_state(Config::default());
        let paths = [
            (
                &state.runtime_paths.active_config,
                fail_runtime,
                b"old-runtime".as_slice(),
            ),
            (
                &state.runtime_paths.node_bindings,
                fail_bindings,
                b"old-bindings".as_slice(),
            ),
        ];
        for (path, fail, _) in &paths {
            if *fail {
                // A directory blocks the atomic file replacement on every platform,
                // including when these tests are run with elevated privileges.
                tokio::fs::create_dir(path).await.unwrap();
            } else {
                tokio::fs::write(path, b"candidate").await.unwrap();
            }
        }

        let result =
            restore_runtime_files(&state, Some(b"old-runtime"), Some(b"old-bindings")).await;
        if fail_runtime || fail_bindings {
            let error = result.unwrap_err().to_string();
            if fail_runtime && fail_bindings {
                assert!(error.contains("Runtime rollback failed:"), "{error}");
                assert!(error.contains("Bindings rollback failed:"), "{error}");
            }
        } else {
            result.unwrap();
        }
        for (path, fail, original) in paths {
            if fail {
                assert!(path.is_dir());
            } else {
                assert_eq!(tokio::fs::read(path).await.unwrap(), original);
            }
        }
    }
}
