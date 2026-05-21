//! Integration tests for [`chum_daemon::Supervisor`].
//!
//! Each test spawns the `fake-mcp.sh` fixture with whatever flags
//! cover the scenario; ZERO real MCP servers run. The fast
//! [`BackoffPolicy`] keeps restart-count assertions under a second.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use chum_core::Manifest;
use chum_core::manifest::{
    Capabilities, Health, Lifecycle, Package, RestartPolicy, Runtime, Source, Transport,
};
use chum_daemon::{
    BackoffPolicy, ProcessKey, ProcessStatus, Supervisor, SupervisorError,
};
use chum_install::{InstalledArtifact, SourceKind};
use tempfile::TempDir;

/// Absolute, canonicalised path to the fake-mcp.sh fixture.
fn fixture_path() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fake-mcp.sh");
    p.canonicalize().expect("fixture should canonicalise")
}

/// Build an artifact + manifest pointing at the fake-mcp fixture
/// with the given args and restart policy. `name` distinguishes
/// keys when a test needs more than one entry in the supervisor.
fn artifact_and_manifest(
    name: &str,
    args: &[&str],
    policy: RestartPolicy,
) -> (InstalledArtifact, Manifest, TempDir) {
    let install_tmp = TempDir::new().expect("install_dir tempdir");
    let install_dir = install_tmp.path().to_path_buf();

    let fixture = fixture_path();
    let artifact = InstalledArtifact {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        install_dir: install_dir.clone(),
        entrypoint: fixture.clone(),
        source_kind: SourceKind::Local,
    };

    let manifest = Manifest {
        schema_version: "0.1".to_string(),
        package: Package {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            description: "test fixture".to_string(),
            license: "MIT".to_string(),
            authors: vec![],
            tags: vec![],
        },
        source: Source::Local {
            path: fixture.display().to_string(),
        },
        runtime: Runtime {
            command: fixture.display().to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            transport: Transport::Stdio,
            env: BTreeMap::new(),
        },
        lifecycle: Lifecycle {
            restart: policy,
            startup_timeout_sec: 10,
            shutdown_grace_sec: 5,
        },
        health: Health::Process,
        capabilities: Capabilities::default(),
        permissions: None,
        signature: None,
    };

    (artifact, manifest, install_tmp)
}

/// Fast backoff for tests so a 3-restart scenario completes in
/// ~350ms instead of ~7s.
fn fast_supervisor() -> Supervisor {
    Supervisor::with_backoff(BackoffPolicy {
        base: Duration::from_millis(50),
        cap: Duration::from_millis(200),
    })
}

/// Poll until `predicate(status)` returns true or `timeout` elapses.
/// Returns the matching status, or panics with the most recent
/// observed value.
async fn wait_until_status<F>(
    supervisor: &Supervisor,
    key: &ProcessKey,
    timeout: Duration,
    mut predicate: F,
) -> ProcessStatus
where
    F: FnMut(&ProcessStatus) -> bool,
{
    let deadline = std::time::Instant::now() + timeout;
    let mut last = None;
    while std::time::Instant::now() < deadline {
        let s = supervisor.status(key).await;
        if let Some(ref status) = s {
            if predicate(status) {
                return status.clone();
            }
            last = Some(status.clone());
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    panic!(
        "timed out waiting for status predicate on {key}; last observed = {last:?}"
    );
}

#[tokio::test]
async fn spawn_runs_child_to_completion() {
    let supervisor = fast_supervisor();
    let (artifact, manifest, _tmp) =
        artifact_and_manifest("clean-exit", &["--exit-code", "0"], RestartPolicy::Never);
    let key = ProcessKey::from_artifact(&artifact);

    let handle = supervisor.spawn(artifact, manifest).await.expect("spawn");
    assert!(handle.pid > 0);
    assert!(handle.started_at <= Utc::now());

    let final_status = wait_until_status(&supervisor, &key, Duration::from_secs(3), |s| {
        s.is_terminal()
    })
    .await;
    assert_eq!(final_status, ProcessStatus::Stopped);
    assert_eq!(supervisor.restart_count(&key).await, Some(0));
}

#[tokio::test]
async fn spawn_duplicate_returns_already_running() {
    let supervisor = fast_supervisor();
    let (artifact, manifest, _tmp) = artifact_and_manifest(
        "long-running",
        &["--exit-after-secs", "10"],
        RestartPolicy::Never,
    );

    let _first = supervisor
        .spawn(artifact.clone(), manifest.clone())
        .await
        .expect("first spawn");

    let err = supervisor
        .spawn(artifact.clone(), manifest.clone())
        .await
        .expect_err("second spawn must fail");
    match err {
        SupervisorError::AlreadyRunning { key } => {
            assert_eq!(key, ProcessKey::from_artifact(&artifact));
        }
        other => panic!("expected AlreadyRunning, got {other:?}"),
    }

    // Clean up — kill the long-running child so test exits promptly.
    let _ = supervisor
        .kill(&ProcessKey::from_artifact(&artifact))
        .await;
}

#[tokio::test]
async fn stop_sigterm_then_sigkill_on_grace_timeout() {
    let supervisor = fast_supervisor();
    // Child ignores SIGTERM, so SIGTERM grace will expire and the
    // supervisor must escalate to SIGKILL.
    let (artifact, manifest, _tmp) = artifact_and_manifest(
        "stubborn",
        &["--ignore-sigterm", "--exit-after-secs", "30"],
        RestartPolicy::Never,
    );
    let key = ProcessKey::from_artifact(&artifact);

    supervisor.spawn(artifact, manifest).await.expect("spawn");
    wait_until_status(&supervisor, &key, Duration::from_secs(2), |s| {
        matches!(s, ProcessStatus::Running)
    })
    .await;

    // Tight grace so SIGKILL must escalate, but generous enough that
    // SIGTERM has time to be ignored.
    let started = std::time::Instant::now();
    supervisor
        .stop(&key, Duration::from_millis(150))
        .await
        .expect("stop must succeed via SIGKILL escalation");
    let elapsed = started.elapsed();

    // SIGKILL escalation should take ~grace plus a small kernel
    // delivery window, comfortably under the 2s hard ceiling.
    assert!(
        elapsed >= Duration::from_millis(140),
        "stop returned suspiciously early: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_millis(1500),
        "stop took too long: {elapsed:?}"
    );

    let status = supervisor.status(&key).await.expect("slot remains");
    assert!(status.is_terminal(), "expected terminal status, got {status:?}");
}

#[tokio::test]
async fn restart_on_failure_when_policy_on_failure() {
    let supervisor = fast_supervisor();
    let (artifact, manifest, _tmp) = artifact_and_manifest(
        "fail-and-restart",
        &["--exit-code", "1"],
        RestartPolicy::OnFailure,
    );
    let key = ProcessKey::from_artifact(&artifact);

    supervisor.spawn(artifact, manifest).await.expect("spawn");

    // Wait until restart_count is at least 2, proving the
    // supervisor saw the failure exit and respawned.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let n = supervisor.restart_count(&key).await.unwrap_or(0);
        if n >= 2 {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("restart_count never reached 2 (last = {n})");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let _ = supervisor.kill(&key).await;
}

#[tokio::test]
async fn no_restart_when_policy_never() {
    let supervisor = fast_supervisor();
    let (artifact, manifest, _tmp) =
        artifact_and_manifest("one-shot", &["--exit-code", "0"], RestartPolicy::Never);
    let key = ProcessKey::from_artifact(&artifact);

    supervisor.spawn(artifact, manifest).await.expect("spawn");
    wait_until_status(&supervisor, &key, Duration::from_secs(3), |s| s.is_terminal()).await;

    // Wait one full backoff window to ensure no respawn would have
    // landed even if the policy were wrong.
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(supervisor.restart_count(&key).await, Some(0));
    assert_eq!(supervisor.status(&key).await, Some(ProcessStatus::Stopped));
}

#[tokio::test]
async fn always_restarts_with_backoff() {
    let supervisor = fast_supervisor();
    // Clean exit + Always policy → keep respawning forever.
    let (artifact, manifest, _tmp) = artifact_and_manifest(
        "always-relaunch",
        &["--exit-code", "0"],
        RestartPolicy::Always,
    );
    let key = ProcessKey::from_artifact(&artifact);

    supervisor.spawn(artifact, manifest).await.expect("spawn");

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let n = supervisor.restart_count(&key).await.unwrap_or(0);
        if n >= 3 {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("restart_count never reached 3 (last = {n})");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let _ = supervisor.kill(&key).await;
}

#[tokio::test]
async fn kill_terminates_immediately() {
    let supervisor = fast_supervisor();
    let (artifact, manifest, _tmp) = artifact_and_manifest(
        "ignore-term",
        &["--ignore-sigterm", "--exit-after-secs", "30"],
        RestartPolicy::Never,
    );
    let key = ProcessKey::from_artifact(&artifact);
    supervisor.spawn(artifact, manifest).await.expect("spawn");

    wait_until_status(&supervisor, &key, Duration::from_secs(2), |s| {
        matches!(s, ProcessStatus::Running)
    })
    .await;

    let started = std::time::Instant::now();
    supervisor.kill(&key).await.expect("kill must succeed");
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_millis(500),
        "kill should be near-instant, took {elapsed:?}"
    );
    assert!(
        supervisor
            .status(&key)
            .await
            .map(|s| s.is_terminal())
            .unwrap_or(false)
    );
}

#[tokio::test]
async fn status_reports_lifecycle_transitions() {
    let supervisor = fast_supervisor();
    let (artifact, manifest, _tmp) = artifact_and_manifest(
        "lifecycle",
        &["--exit-after-secs", "1", "--exit-code", "0"],
        RestartPolicy::Never,
    );
    let key = ProcessKey::from_artifact(&artifact);

    supervisor.spawn(artifact, manifest).await.expect("spawn");

    // Expect Running first.
    wait_until_status(&supervisor, &key, Duration::from_secs(2), |s| {
        matches!(s, ProcessStatus::Running)
    })
    .await;

    // And then Stopped after the child exits cleanly.
    let final_status =
        wait_until_status(&supervisor, &key, Duration::from_secs(3), |s| s.is_terminal()).await;
    assert_eq!(final_status, ProcessStatus::Stopped);

    // list() should show the same key with the same terminal status.
    let listed = supervisor.list().await;
    assert!(
        listed
            .iter()
            .any(|(k, s)| k == &key && s == &ProcessStatus::Stopped),
        "list missing terminal entry: {listed:?}"
    );
}

#[tokio::test]
async fn drop_does_not_panic_with_live_children() {
    let pid = {
        let supervisor = fast_supervisor();
        let (artifact, manifest, _tmp) = artifact_and_manifest(
            "drop-test",
            &["--exit-after-secs", "30"],
            RestartPolicy::Never,
        );
        let handle = supervisor.spawn(artifact, manifest).await.expect("spawn");
        // Give the child a moment to actually start.
        tokio::time::sleep(Duration::from_millis(50)).await;
        // _tmp + supervisor are dropped at the end of this scope, exercising
        // the Drop impl while a child is alive. SIGKILL fires via nix, and
        // monitor tasks abort; this test asserts that the path doesn't
        // panic and that the test process exits cleanly afterward.
        handle.pid
    };
    assert!(pid > 0);

    // Yield long enough for the monitor task to be aborted and for
    // kill_on_drop to fire on the underlying tokio::process::Child.
    tokio::time::sleep(Duration::from_millis(200)).await;
    // No PID assertion here — see process.rs comment on the pid-reuse
    // race + zombie reaping semantics in tokio::process. The property
    // tested is the absence of panic / deadlock from Supervisor::drop.
}

#[tokio::test]
async fn restart_replaces_running_child() {
    let supervisor = fast_supervisor();
    let (artifact, manifest, _tmp) = artifact_and_manifest(
        "restartable",
        &["--exit-after-secs", "10"],
        RestartPolicy::Never,
    );
    let key = ProcessKey::from_artifact(&artifact);

    let first = supervisor
        .spawn(artifact.clone(), manifest.clone())
        .await
        .expect("first spawn");
    wait_until_status(&supervisor, &key, Duration::from_secs(2), |s| {
        matches!(s, ProcessStatus::Running)
    })
    .await;

    let second = supervisor.restart(&key).await.expect("restart");
    assert_ne!(first.pid, second.pid, "restart must spawn a new pid");

    let _ = supervisor.kill(&key).await;
}
