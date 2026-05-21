//! Integration tests for the `chumd` binary.
//!
//! Each test spawns `chumd` as a subprocess with a per-test
//! `--socket-path` (under a tempdir) and drives it via the public
//! [`chum_daemon::DaemonClient`]. The chumd process is SIGTERM'd at
//! the end of every test so the socket file is cleaned up.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use chum_daemon::ipc::{PROTOCOL_VERSION, Request, Response, codes};
use chum_daemon::{DaemonClient, IpcError};
use tempfile::TempDir;
use tokio::net::UnixStream;
use tokio::process::{Child, Command};

const READY_POLL_INTERVAL: Duration = Duration::from_millis(50);
const READY_TIMEOUT: Duration = Duration::from_secs(2);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

/// Handle to a running chumd subprocess + the per-test scratch.
struct Chumd {
    _tmp: TempDir,
    root: PathBuf,
    socket: PathBuf,
    child: Child,
}

impl Chumd {
    /// Spawn chumd against a fresh tempdir-rooted CHUM_HOME.
    async fn spawn() -> Self {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path().to_path_buf();
        let socket = root.join("daemon.sock");
        let bin = env!("CARGO_BIN_EXE_chumd");

        let child = Command::new(bin)
            .arg("--root")
            .arg(&root)
            .arg("--socket-path")
            .arg(&socket)
            .kill_on_drop(true)
            .spawn()
            .expect("spawn chumd");

        let me = Self {
            _tmp: tmp,
            root,
            socket,
            child,
        };
        me.wait_until_ready().await;
        me
    }

    /// Spawn chumd at a pre-existing socket path (used by the
    /// zombie-socket test). The caller is responsible for creating
    /// the stale entry beforehand.
    async fn spawn_at(tmp: TempDir, socket: PathBuf) -> Self {
        let root = tmp.path().to_path_buf();
        let bin = env!("CARGO_BIN_EXE_chumd");

        let child = Command::new(bin)
            .arg("--root")
            .arg(&root)
            .arg("--socket-path")
            .arg(&socket)
            .kill_on_drop(true)
            .spawn()
            .expect("spawn chumd");

        let me = Self {
            _tmp: tmp,
            root,
            socket,
            child,
        };
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

    fn pid(&self) -> i32 {
        self.child.id().expect("chumd should have a pid") as i32
    }

    /// Send SIGTERM and wait for the child to exit.
    async fn sigterm_and_wait(mut self) -> std::process::ExitStatus {
        let pid = self.pid();
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGTERM,
        )
        .expect("SIGTERM");
        match tokio::time::timeout(SHUTDOWN_TIMEOUT, self.child.wait()).await {
            Ok(Ok(status)) => status,
            Ok(Err(e)) => panic!("wait() failed: {e}"),
            Err(_) => panic!("chumd did not exit within {SHUTDOWN_TIMEOUT:?}"),
        }
    }
}

#[tokio::test]
async fn ping_returns_expected_envelope() {
    let chumd = Chumd::spawn().await;
    let ping = chumd.client().ping().await.expect("ping");
    assert_eq!(ping.daemon_version, chum_daemon::DAEMON_VERSION);
    assert_eq!(ping.installed_count, 0);
    // uptime_secs is u64; just confirm the field exists by reading it.
    let _ = ping.uptime_secs;
    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn status_returns_expected_envelope() {
    let chumd = Chumd::spawn().await;
    let expected_pid = chumd.pid() as u32;
    let status = chumd.client().status().await.expect("status");
    assert_eq!(status.pid, expected_pid);
    assert_eq!(status.installed_count, 0);
    assert_eq!(status.running_count, 0);
    // started_at must be valid RFC3339.
    chrono::DateTime::parse_from_rfc3339(&status.started_at)
        .expect("started_at parses as RFC3339");
    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn list_processes_returns_empty_array() {
    let chumd = Chumd::spawn().await;
    let listed = chumd
        .client()
        .list_processes()
        .await
        .expect("list_processes");
    assert!(listed.processes.is_empty(), "v0.1 list is always empty");
    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn unknown_verb_returns_unknown_verb_code() {
    let chumd = Chumd::spawn().await;
    let req = Request {
        protocol_version: PROTOCOL_VERSION,
        verb: "definitely_not_a_verb".to_string(),
        args: serde_json::Value::Null,
    };
    let resp = chumd.client().request(&req).await.expect("request");
    match resp {
        Response::Error { code, .. } => assert_eq!(code, codes::UNKNOWN_VERB),
        Response::Ok { .. } => panic!("expected error response, got Ok"),
    }
    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn unsupported_protocol_version_returns_code() {
    let chumd = Chumd::spawn().await;
    let req = Request {
        protocol_version: 99,
        verb: "ping".to_string(),
        args: serde_json::Value::Null,
    };
    let resp = chumd.client().request(&req).await.expect("request");
    match resp {
        Response::Error { code, .. } => {
            assert_eq!(code, codes::UNSUPPORTED_PROTOCOL_VERSION);
        }
        Response::Ok { .. } => panic!("expected error, got Ok"),
    }
    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn zombie_socket_recovered_on_startup() {
    let tmp = TempDir::new().expect("tempdir");
    let socket = tmp.path().join("daemon.sock");
    // Touch a regular file at the socket location to simulate a
    // SIGKILL'd previous chumd that didn't get to remove its socket.
    std::fs::write(&socket, b"stale").expect("touch stale socket");
    assert!(socket.exists());

    let chumd = Chumd::spawn_at(tmp, socket).await;
    let ping = chumd
        .client()
        .ping()
        .await
        .expect("ping must succeed after zombie recovery");
    assert_eq!(ping.installed_count, 0);
    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn sigterm_removes_socket_file() {
    let chumd = Chumd::spawn().await;
    assert!(chumd.socket.exists(), "socket must exist while chumd runs");
    let socket = chumd.socket.clone();
    let status = chumd.sigterm_and_wait().await;
    assert!(
        status.success(),
        "chumd should exit 0 on SIGTERM, got {status:?}"
    );
    assert!(
        !socket.exists(),
        "socket file must be removed during graceful shutdown"
    );
}

#[tokio::test]
async fn second_chumd_against_live_socket_fails_fast() {
    let chumd = Chumd::spawn().await;

    // Spawn a second chumd pointing at the same live socket. It must
    // refuse to start (zombie check sees a real listener).
    let bin = env!("CARGO_BIN_EXE_chumd");
    let second = Command::new(bin)
        .arg("--root")
        .arg(&chumd.root)
        .arg("--socket-path")
        .arg(&chumd.socket)
        .kill_on_drop(true)
        .output()
        .await
        .expect("spawn second chumd");
    assert!(
        !second.status.success(),
        "second chumd must exit non-zero when socket is in use"
    );
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("appears to be running")
            || stderr.contains("SocketAlreadyInUse")
            || stderr.contains("socket"),
        "expected explanatory message about live chumd, got: {stderr}"
    );

    // Original chumd still works.
    chumd.client().ping().await.expect("original still alive");
    chumd.sigterm_and_wait().await;
}

#[tokio::test]
async fn client_against_missing_socket_returns_connect_failed() {
    let tmp = TempDir::new().expect("tempdir");
    let socket = tmp.path().join("does-not-exist.sock");
    let client = DaemonClient::new(socket);
    let err = client.ping().await.expect_err("must error");
    match err {
        IpcError::ConnectFailed { .. } => {}
        other => panic!("expected ConnectFailed, got {other:?}"),
    }
}
