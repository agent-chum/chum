//! Integration tests for the lifecycle verbs: `spawn`, `terminate`,
//! `restart`, `process_status`, and the extended `list_processes`.
//!
//! Each test spawns a fresh `chumd` against a per-test tempdir that
//! has been pre-populated with a registry row + `chum-manifest.toml`
//! pointing at the `fake-mcp.sh` fixture. ZERO real MCP servers run.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chum_daemon::DaemonClient;
use chum_install::{InstalledArtifact, SourceKind};
use chum_registry::Registry;
use tempfile::TempDir;
use tokio::net::UnixStream;
use tokio::process::{Child, Command};

const READY_POLL_INTERVAL: Duration = Duration::from_millis(50);
const READY_TIMEOUT: Duration = Duration::from_secs(2);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

/// Canonical absolute path to the existing fake-mcp.sh fixture
/// (re-used from the supervisor session — see
/// `tests/fixtures/fake-mcp.sh`).
fn fake_mcp_path() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fake-mcp.sh");
    p.canonicalize().expect("fake-mcp fixture should canonicalise")
}

/// One installed package, ready for the daemon to spawn against.
/// Drops the tempdir on `Drop`.
struct InstalledFixture {
    _tmp: TempDir,
    chum_root: PathBuf,
    install_dir: PathBuf,
    name: String,
    version: String,
}

impl InstalledFixture {
    /// Build a fixture for a given name/version that, when spawned,
    /// runs `fake-mcp.sh` with `args`.
    fn new(name: &str, version: &str, args: &[&str]) -> Self {
        let tmp = TempDir::new().expect("tempdir");
        let chum_root = tmp.path().to_path_buf();
        let install_dir = chum_root
            .join("packages")
            .join(name)
            .join(version);
        std::fs::create_dir_all(&install_dir).expect("create install_dir");
        std::fs::create_dir_all(install_dir.join("logs"))
            .expect("create logs dir");

        let fake_mcp = fake_mcp_path();
        let symlink = install_dir.join("local-src");
        std::os::unix::fs::symlink(&fake_mcp, &symlink).expect("symlink local-src");

        let args_toml = args
            .iter()
            .map(|a| format!("\"{a}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let manifest = format!(
            r#"schema_version = "0.1"

[package]
name = "{name}"
version = "{version}"
description = "lifecycle test fixture"
license = "MIT"
authors = []

[source]
kind = "local"
path = "{fake_mcp}"

[runtime]
command = "{fake_mcp}"
args = [{args_toml}]

[runtime.transport]
kind = "stdio"

[lifecycle]
restart = "never"
"#,
            name = name,
            version = version,
            fake_mcp = fake_mcp.display(),
            args_toml = args_toml,
        );
        std::fs::write(install_dir.join("chum-manifest.toml"), &manifest)
            .expect("write chum-manifest.toml");

        // Insert the registry row so the daemon can resolve
        // install_dir from (name, version).
        let registry = Registry::open(chum_root.join("state.db")).expect("open registry");
        let artifact = InstalledArtifact {
            name: name.to_string(),
            version: version.to_string(),
            install_dir: install_dir.clone(),
            entrypoint: symlink,
            source_kind: SourceKind::Local,
        };
        registry.insert(&artifact).expect("insert registry row");

        Self {
            _tmp: tmp,
            chum_root,
            install_dir,
            name: name.to_string(),
            version: version.to_string(),
        }
    }
}

/// Handle to a running chumd subprocess.
struct Chumd {
    socket: PathBuf,
    child: Child,
}

impl Chumd {
    async fn spawn_at(chum_root: &Path) -> Self {
        let socket = chum_root.join("daemon.sock");
        let bin = env!("CARGO_BIN_EXE_chumd");
        let child = Command::new(bin)
            .arg("--root")
            .arg(chum_root)
            .arg("--socket-path")
            .arg(&socket)
            .kill_on_drop(true)
            .spawn()
            .expect("spawn chumd");

        let me = Self { socket, child };
        me.wait_until_ready().await;
        me
    }

    async fn wait_until_ready(&self) {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if UnixStream::connect(&self.socket).await.is_ok() {
                return;
            }
            tokio::time::sleep(READY_POLL_INTERVAL).await;
        }
        panic!(
            "chumd never became ready on {} within {READY_TIMEOUT:?}",
            self.socket.display()
        );
    }

    fn client(&self) -> DaemonClient {
        DaemonClient::new(self.socket.clone())
    }

    async fn sigterm_and_wait(mut self) {
        if let Some(pid) = self.child.id() {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGTERM,
            );
        }
        let _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, self.child.wait()).await;
    }
}

/// Poll `process_status` until the predicate matches or the timeout
/// expires. Panics with the most recent observed status on failure.
async fn wait_until_status<F>(
    client: &DaemonClient,
    name: &str,
    version: &str,
    timeout: Duration,
    mut predicate: F,
) -> String
where
    F: FnMut(&str) -> bool,
{
    let deadline = Instant::now() + timeout;
    let mut last = String::new();
    while Instant::now() < deadline {
        match client.process_status(name, version).await {
            Ok(resp) => {
                if predicate(&resp.status) {
                    return resp.status;
                }
                last = resp.status;
            }
            Err(e) => last = format!("err: {e}"),
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting on status predicate; last = {last}");
}

#[tokio::test]
async fn install_start_status_stop_cycle() {
    let fixture = InstalledFixture::new(
        "lifecycle-cycle",
        "0.1.0",
        &["--exit-after-secs", "30"],
    );
    let chumd = Chumd::spawn_at(&fixture.chum_root).await;
    let client = chumd.client();

    // Pre-start: status should be "stopped" (in registry, not in supervisor).
    let pre = client
        .process_status(&fixture.name, &fixture.version)
        .await
        .expect("pre-start status");
    assert_eq!(pre.status, "stopped");
    assert_eq!(pre.pid, None);
    assert_eq!(pre.restart_count, 0);

    // Start.
    let spawned = client
        .spawn_process(&fixture.name, &fixture.version)
        .await
        .expect("spawn");
    assert!(spawned.pid > 0);
    chrono::DateTime::parse_from_rfc3339(&spawned.started_at)
        .expect("started_at is RFC3339");

    // Status flips to running.
    wait_until_status(
        &client,
        &fixture.name,
        &fixture.version,
        Duration::from_secs(2),
        |s| s == "running",
    )
    .await;

    // Log files exist.
    assert!(
        fixture.install_dir.join("logs").join("stdout.log").is_file(),
        "stdout.log not created by spawn"
    );
    assert!(
        fixture.install_dir.join("logs").join("stderr.log").is_file(),
        "stderr.log not created by spawn"
    );

    // Stop with default grace.
    let stopped = client
        .terminate_process(&fixture.name, &fixture.version, None)
        .await
        .expect("terminate");
    assert!(stopped.stopped);

    // Status reports terminal.
    let post = client
        .process_status(&fixture.name, &fixture.version)
        .await
        .expect("post-stop status");
    assert!(
        post.status == "stopped" || post.status == "failed",
        "expected terminal status, got {}",
        post.status
    );

    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn start_already_running_returns_error() {
    let fixture = InstalledFixture::new(
        "already-running",
        "0.1.0",
        &["--exit-after-secs", "30"],
    );
    let chumd = Chumd::spawn_at(&fixture.chum_root).await;
    let client = chumd.client();

    client
        .spawn_process(&fixture.name, &fixture.version)
        .await
        .expect("first spawn");
    wait_until_status(&client, &fixture.name, &fixture.version, Duration::from_secs(2), |s| s == "running").await;

    let err = client
        .spawn_process(&fixture.name, &fixture.version)
        .await
        .expect_err("second spawn must fail");
    match err {
        chum_daemon::IpcError::ServerError { code, .. } => {
            assert_eq!(code, chum_daemon::codes::PROCESS_ALREADY_RUNNING);
        }
        other => panic!("expected ServerError with process_already_running, got {other:?}"),
    }

    let _ = client.terminate_process(&fixture.name, &fixture.version, None).await;
    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn stop_not_running_returns_error() {
    let fixture = InstalledFixture::new(
        "stop-not-running",
        "0.1.0",
        &["--exit-after-secs", "30"],
    );
    let chumd = Chumd::spawn_at(&fixture.chum_root).await;
    let client = chumd.client();

    // No spawn first — terminate should fail.
    let err = client
        .terminate_process(&fixture.name, &fixture.version, None)
        .await
        .expect_err("terminate must fail without prior spawn");
    match err {
        chum_daemon::IpcError::ServerError { code, .. } => {
            assert_eq!(code, chum_daemon::codes::PROCESS_NOT_RUNNING);
        }
        other => panic!("expected ServerError with process_not_running, got {other:?}"),
    }

    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn restart_increments_count() {
    let fixture = InstalledFixture::new(
        "restart-counter",
        "0.1.0",
        &["--exit-after-secs", "30"],
    );
    let chumd = Chumd::spawn_at(&fixture.chum_root).await;
    let client = chumd.client();

    client.spawn_process(&fixture.name, &fixture.version).await.expect("spawn");
    wait_until_status(&client, &fixture.name, &fixture.version, Duration::from_secs(2), |s| s == "running").await;

    let r1 = client.restart_process(&fixture.name, &fixture.version).await.expect("restart 1");
    assert_eq!(r1.restart_count, 1);

    wait_until_status(&client, &fixture.name, &fixture.version, Duration::from_secs(2), |s| s == "running").await;

    let r2 = client.restart_process(&fixture.name, &fixture.version).await.expect("restart 2");
    assert_eq!(r2.restart_count, 2);
    assert_ne!(r1.pid, r2.pid, "each restart must produce a new pid");

    // process_status reflects the same count.
    let status = client.process_status(&fixture.name, &fixture.version).await.expect("status");
    assert_eq!(status.restart_count, 2);

    let _ = client.terminate_process(&fixture.name, &fixture.version, None).await;
    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn process_status_unknown_returns_not_installed() {
    let tmp = TempDir::new().expect("tempdir");
    let chumd = Chumd::spawn_at(tmp.path()).await;
    let client = chumd.client();

    let err = client
        .process_status("does-not-exist", "9.9.9")
        .await
        .expect_err("unknown name must fail");
    match err {
        chum_daemon::IpcError::ServerError { code, .. } => {
            assert_eq!(code, chum_daemon::codes::PROCESS_NOT_INSTALLED);
        }
        other => panic!("expected process_not_installed, got {other:?}"),
    }

    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn list_processes_shows_running() {
    let fixture = InstalledFixture::new(
        "list-shows-it",
        "0.1.0",
        &["--exit-after-secs", "30"],
    );
    let chumd = Chumd::spawn_at(&fixture.chum_root).await;
    let client = chumd.client();

    // Before spawn — list is empty.
    let pre = client.list_processes().await.expect("list_processes pre");
    assert!(pre.processes.is_empty(), "list should be empty before spawn");

    client.spawn_process(&fixture.name, &fixture.version).await.expect("spawn");
    wait_until_status(&client, &fixture.name, &fixture.version, Duration::from_secs(2), |s| s == "running").await;

    let listed = client.list_processes().await.expect("list_processes after spawn");
    assert_eq!(listed.processes.len(), 1);
    let entry = &listed.processes[0];
    assert_eq!(entry.name, fixture.name);
    assert_eq!(entry.version, fixture.version);
    assert_eq!(entry.status, "running");
    assert!(entry.pid.is_some_and(|p| p > 0));
    assert_eq!(entry.restart_count, 0);
    assert_eq!(entry.exit_code, None);

    let _ = client.terminate_process(&fixture.name, &fixture.version, None).await;
    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn spawn_missing_manifest_returns_specific_code() {
    // Construct a registry row without writing the manifest file.
    // This simulates an install from before this session landed.
    let tmp = TempDir::new().expect("tempdir");
    let chum_root = tmp.path().to_path_buf();
    let install_dir = chum_root.join("packages/orphaned/0.1.0");
    std::fs::create_dir_all(&install_dir).expect("mkdir install_dir");
    // Intentionally do NOT write chum-manifest.toml.
    let registry = Registry::open(chum_root.join("state.db")).expect("registry");
    let artifact = InstalledArtifact {
        name: "orphaned".to_string(),
        version: "0.1.0".to_string(),
        install_dir,
        entrypoint: PathBuf::from("/usr/bin/true"),
        source_kind: SourceKind::Local,
    };
    registry.insert(&artifact).expect("insert");

    let chumd = Chumd::spawn_at(&chum_root).await;
    let client = chumd.client();

    let err = client
        .spawn_process("orphaned", "0.1.0")
        .await
        .expect_err("spawn must fail");
    match err {
        chum_daemon::IpcError::ServerError { code, .. } => {
            assert_eq!(code, chum_daemon::codes::MANIFEST_MISSING_IN_INSTALL_DIR);
        }
        other => panic!("expected manifest_missing_in_install_dir, got {other:?}"),
    }

    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn logs_returns_recent_lines() {
    let fixture = InstalledFixture::new(
        "logs-fixture",
        "0.1.0",
        &[
            "--print-to-stdout",
            "hello-stdout",
            "--print-to-stderr",
            "hello-stderr",
            "--exit-after-secs",
            "0",
        ],
    );
    let chumd = Chumd::spawn_at(&fixture.chum_root).await;
    let client = chumd.client();

    client.spawn_process(&fixture.name, &fixture.version).await.expect("spawn");
    // The fake-mcp exits immediately after the prints; wait for terminal.
    wait_until_status(&client, &fixture.name, &fixture.version, Duration::from_secs(2), |s| {
        matches!(s, "stopped" | "failed")
    })
    .await;

    let resp = client
        .tail_logs(&fixture.name, &fixture.version, "both", 100)
        .await
        .expect("tail_logs both");
    assert_eq!(resp.stream, "both");
    assert!(
        resp.content.contains("hello-stdout"),
        "missing stdout line in: {}",
        resp.content
    );
    assert!(
        resp.content.contains("hello-stderr"),
        "missing stderr line in: {}",
        resp.content
    );
    assert!(
        resp.content.contains("=== stdout.log"),
        "missing stdout header in `both` mode: {}",
        resp.content
    );

    // Stream-specific queries.
    let just_stdout = client
        .tail_logs(&fixture.name, &fixture.version, "stdout", 100)
        .await
        .expect("tail_logs stdout");
    assert!(just_stdout.content.contains("hello-stdout"));
    assert!(!just_stdout.content.contains("hello-stderr"));

    let just_stderr = client
        .tail_logs(&fixture.name, &fixture.version, "stderr", 100)
        .await
        .expect("tail_logs stderr");
    assert!(just_stderr.content.contains("hello-stderr"));
    assert!(!just_stderr.content.contains("hello-stdout"));

    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn logs_respects_lines_cap() {
    let fixture = InstalledFixture::new(
        "logs-cap",
        "0.1.0",
        &["--exit-after-secs", "0"],
    );
    let chumd = Chumd::spawn_at(&fixture.chum_root).await;
    let client = chumd.client();

    let err = client
        .tail_logs(&fixture.name, &fixture.version, "both", 20_000)
        .await
        .expect_err("lines > 10_000 must fail");
    match err {
        chum_daemon::IpcError::ServerError { code, .. } => {
            assert_eq!(code, chum_daemon::codes::LOGS_LINES_TOO_LARGE);
        }
        other => panic!("expected logs_lines_too_large, got {other:?}"),
    }

    // Zero lines also rejected.
    let zero = client
        .tail_logs(&fixture.name, &fixture.version, "both", 0)
        .await
        .expect_err("lines = 0 must fail");
    match zero {
        chum_daemon::IpcError::ServerError { code, .. } => {
            assert_eq!(code, chum_daemon::codes::LOGS_LINES_TOO_LARGE);
        }
        other => panic!("expected logs_lines_too_large for 0, got {other:?}"),
    }

    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn logs_missing_returns_specific_error() {
    // Install but never start — log files don't exist yet.
    let fixture = InstalledFixture::new(
        "logs-missing",
        "0.1.0",
        &["--exit-after-secs", "0"],
    );
    // Wipe the logs/ dir to simulate "no logs ever produced".
    let _ = std::fs::remove_dir_all(fixture.install_dir.join("logs"));

    let chumd = Chumd::spawn_at(&fixture.chum_root).await;
    let client = chumd.client();

    let err = client
        .tail_logs(&fixture.name, &fixture.version, "both", 100)
        .await
        .expect_err("missing logs must error");
    match err {
        chum_daemon::IpcError::ServerError { code, message } => {
            assert_eq!(code, chum_daemon::codes::LOGS_UNAVAILABLE);
            assert!(
                message.contains("start it"),
                "message should hint at chum start: {message}"
            );
        }
        other => panic!("expected logs_unavailable, got {other:?}"),
    }

    // Unknown stream string also rejected with its own code.
    let bad_stream = client
        .tail_logs(&fixture.name, &fixture.version, "neither", 100)
        .await;
    // logs/ is missing so the daemon will return LOGS_UNAVAILABLE before
    // it gets a chance to validate the stream name — but if the user
    // pre-created the logs/ dir, the invalid-stream check kicks in.
    // Both outcomes are acceptable for this assertion; we just verify
    // we get a server error and not a transport error.
    match bad_stream {
        Err(chum_daemon::IpcError::ServerError { code, .. }) => {
            assert!(
                code == chum_daemon::codes::LOGS_INVALID_STREAM
                    || code == chum_daemon::codes::LOGS_UNAVAILABLE,
                "unexpected code for invalid stream: {code}"
            );
        }
        Err(other) => panic!("expected ServerError, got {other:?}"),
        Ok(_) => panic!("expected error, got Ok"),
    }

    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn spawn_with_unmet_permission_returns_permission_denied() {
    let tmp = TempDir::new().unwrap();
    let chum_root = tmp.path().to_path_buf();
    let install_dir = chum_root
        .join("packages")
        .join("broker-test")
        .join("0.1.0");
    std::fs::create_dir_all(&install_dir).unwrap();
    std::fs::create_dir_all(install_dir.join("logs")).unwrap();

    let fake_mcp = fake_mcp_path();
    let symlink = install_dir.join("local-src");
    std::os::unix::fs::symlink(&fake_mcp, &symlink).unwrap();

    // Manifest declares a permission the user has not granted.
    let manifest = format!(
        r#"schema_version = "0.1"

[package]
name = "broker-test"
version = "0.1.0"
description = "broker integration test fixture"
license = "MIT"
authors = []

[source]
kind = "local"
path = "{fake_mcp}"

[runtime]
command = "{fake_mcp}"
args = ["--exit-after-secs", "30"]

[runtime.transport]
kind = "stdio"

[lifecycle]
restart = "never"

[permissions.env]
read = ["BROKER_TEST_KEY"]
"#,
        fake_mcp = fake_mcp.display(),
    );
    std::fs::write(install_dir.join("chum-manifest.toml"), &manifest).unwrap();

    let registry = Registry::open(chum_root.join("state.db")).unwrap();
    let artifact_id = registry
        .insert(&InstalledArtifact {
            name: "broker-test".to_string(),
            version: "0.1.0".to_string(),
            install_dir: install_dir.clone(),
            entrypoint: symlink,
            source_kind: SourceKind::Local,
        })
        .unwrap();

    let chumd = Chumd::spawn_at(&chum_root).await;
    let client = chumd.client();

    // First spawn: no grants yet → permission_denied with the unmet
    // requirement spelled out.
    let err = client
        .spawn_process("broker-test", "0.1.0")
        .await
        .expect_err("spawn must fail without grant");
    match err {
        chum_daemon::IpcError::ServerError { code, message } => {
            assert_eq!(code, chum_daemon::codes::PERMISSION_DENIED);
            assert!(
                message.contains("env.read=BROKER_TEST_KEY"),
                "message should name the unmet requirement: {message}"
            );
            assert!(
                message.contains("chum permit"),
                "message should hint at chum permit: {message}"
            );
        }
        other => panic!("expected permission_denied, got {other:?}"),
    }

    // Issue the grant out-of-band (the cli's chum permit would do this).
    registry
        .grant(artifact_id, "env.read", "BROKER_TEST_KEY")
        .unwrap();

    // Now spawn succeeds.
    let spawned = client
        .spawn_process("broker-test", "0.1.0")
        .await
        .expect("spawn must succeed after grant");
    assert!(spawned.pid > 0);

    let _ = client.terminate_process("broker-test", "0.1.0", None).await;
    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn spawn_with_empty_permissions_passes_broker() {
    // Default-empty Permissions (manifest has no [permissions] block)
    // — broker auto-allows. The existing install_start_status_stop_cycle
    // already exercises this implicitly, but a focused test makes the
    // "pre-broker manifests still work" invariant bisectable.
    let fixture = InstalledFixture::new(
        "broker-allow",
        "0.1.0",
        &["--exit-after-secs", "30"],
    );
    let chumd = Chumd::spawn_at(&fixture.chum_root).await;
    let client = chumd.client();

    let spawned = client
        .spawn_process(&fixture.name, &fixture.version)
        .await
        .expect("spawn with empty permissions must succeed");
    assert!(spawned.pid > 0);

    let _ = client.terminate_process(&fixture.name, &fixture.version, None).await;
    chumd.sigterm_and_wait().await;
}
