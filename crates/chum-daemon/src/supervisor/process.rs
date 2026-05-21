//! Child-process spawn + monitor task.
//!
//! Each `Supervisor::spawn` allocates a tokio task that owns the
//! [`tokio::process::Child`] and drives its lifecycle. The task
//! observes shutdown and restart-policy signals via shared atomics
//! and a watch channel; the supervisor API never touches the `Child`
//! directly — it signals by PID via `nix`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use chum_core::Manifest;
use chum_install::InstalledArtifact;
use tokio::process::{Child, Command};
use tokio::sync::{RwLock, watch};

use crate::error::SupervisorError;
use crate::supervisor::restart::{BackoffPolicy, should_restart};
use crate::supervisor::{ProcessKey, ProcessStatus};

/// Shared state that the supervisor map holds for each process and
/// the monitor task reads/writes.
///
/// Cloned by [`MonitorContext::handles`] for use from the supervisor
/// API without holding the map lock across `await` points.
pub(crate) struct ProcessSlot {
    pub(crate) artifact: InstalledArtifact,
    pub(crate) manifest: Manifest,
    pub(crate) pid: Arc<AtomicI32>,
    pub(crate) shutdown: Arc<AtomicBool>,
    pub(crate) restart_count: Arc<AtomicU32>,
    pub(crate) status_rx: watch::Receiver<ProcessStatus>,
    pub(crate) monitor: tokio::task::JoinHandle<()>,
}

/// A non-`JoinHandle` snapshot of the shared state, safe to pass
/// across `await` points after releasing the map lock.
#[derive(Clone)]
pub(crate) struct SlotHandles {
    pub(crate) artifact: InstalledArtifact,
    pub(crate) manifest: Manifest,
    pub(crate) pid: Arc<AtomicI32>,
    pub(crate) shutdown: Arc<AtomicBool>,
    pub(crate) status_rx: watch::Receiver<ProcessStatus>,
}

impl ProcessSlot {
    pub(crate) fn handles(&self) -> SlotHandles {
        SlotHandles {
            artifact: self.artifact.clone(),
            manifest: self.manifest.clone(),
            pid: self.pid.clone(),
            shutdown: self.shutdown.clone(),
            status_rx: self.status_rx.clone(),
        }
    }
}

/// Configuration the monitor task needs to drive a process.
pub(crate) struct MonitorContext {
    pub(crate) key: ProcessKey,
    pub(crate) artifact: InstalledArtifact,
    pub(crate) manifest: Manifest,
    pub(crate) pid: Arc<AtomicI32>,
    pub(crate) started_at: Arc<RwLock<DateTime<Utc>>>,
    pub(crate) shutdown: Arc<AtomicBool>,
    pub(crate) restart_count: Arc<AtomicU32>,
    pub(crate) status_tx: watch::Sender<ProcessStatus>,
    pub(crate) backoff: BackoffPolicy,
}

/// Spawn the child process described by `manifest.runtime` inside
/// `artifact.install_dir`. Returns the live `Child` handle on success.
///
/// stdout / stderr are redirected to per-package log files under
/// `<install_dir>/logs/{stdout,stderr}.log` (created on first spawn
/// if missing — `chum-install` also pre-creates `logs/` so a fresh
/// install lands writable immediately).
///
/// Sets `kill_on_drop(true)` so a panicking monitor task does not
/// leave an orphan even before [`Supervisor`]'s `Drop` runs.
// TODO(chum-v0.x): structured log streaming for `chum logs` lands
// in Session B.5 — today the daemon only writes files; tail / follow
// support requires a separate IPC channel that's out of v0.1 scope.
//
// TODO(chum-v0.2): log rotation. v0.1 appends forever; long-running
// servers will accumulate log files until the disk is full. Needs:
//   - cap file size (e.g. 10 MB per file)
//   - truncate-to-tail-1MB on overflow OR proper numbered rotation
//     (chum.stdout.log → chum.stdout.log.1 → chum.stdout.log.2 …)
//   - integration with `chum logs` so historical rotated files can
//     still be read
// Naive truncate-to-tail loses log lines and breaks `chum logs` —
// numbered rotation is the right shape but needs a flush boundary
// and probably a separate writer task. Deferred to v0.2 where the
// streaming-logs work makes the rewriter live anyway.
pub(crate) fn spawn_child(
    artifact: &InstalledArtifact,
    manifest: &Manifest,
) -> Result<Child, SupervisorError> {
    let logs_dir = artifact.install_dir.join("logs");
    std::fs::create_dir_all(&logs_dir).map_err(SupervisorError::Io)?;
    let stdout_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs_dir.join("stdout.log"))
        .map_err(SupervisorError::Io)?;
    let stderr_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs_dir.join("stderr.log"))
        .map_err(SupervisorError::Io)?;

    let mut cmd = Command::new(&manifest.runtime.command);
    cmd.args(&manifest.runtime.args)
        .envs(&manifest.runtime.env)
        .current_dir(&artifact.install_dir)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::from(stdout_file))
        .stderr(std::process::Stdio::from(stderr_file));

    cmd.spawn()
        .map_err(|source| SupervisorError::SpawnFailed { source })
}

/// Body of the monitor task. Owns the `Child`, drives the
/// lifecycle, applies restart policy with backoff, and writes
/// status transitions through `status_tx`. Ends when the process
/// reaches a terminal state (Stopped / Failed).
pub(crate) async fn monitor_loop(ctx: MonitorContext, mut child: Child) {
    let MonitorContext {
        key: _key,
        artifact,
        manifest,
        pid,
        started_at,
        shutdown,
        restart_count,
        status_tx,
        backoff,
    } = ctx;

    pid.store(child_pid(&child), Ordering::SeqCst);
    *started_at.write().await = Utc::now();
    let _ = status_tx.send(ProcessStatus::Running);

    loop {
        let exit = child.wait().await;
        pid.store(-1, Ordering::SeqCst);

        if shutdown.load(Ordering::SeqCst) {
            let _ = status_tx.send(ProcessStatus::Stopped);
            return;
        }

        let exit_code = exit.as_ref().ok().and_then(|s| s.code());
        let restart = should_restart(manifest.lifecycle.restart, exit_code);

        if !restart {
            let final_status = match exit_code {
                Some(0) => ProcessStatus::Stopped,
                Some(code) => ProcessStatus::Failed { exit_code: code },
                None => ProcessStatus::Failed { exit_code: -1 },
            };
            let _ = status_tx.send(final_status);
            return;
        }

        let attempt = restart_count.fetch_add(1, Ordering::SeqCst) + 1;
        let delay = backoff.delay_for(attempt);
        let _ = status_tx.send(ProcessStatus::Restarting);
        tokio::time::sleep(delay).await;

        // Shutdown may have been requested during the sleep.
        if shutdown.load(Ordering::SeqCst) {
            let _ = status_tx.send(ProcessStatus::Stopped);
            return;
        }

        let _ = status_tx.send(ProcessStatus::Starting);
        child = match spawn_child(&artifact, &manifest) {
            Ok(c) => c,
            Err(_) => {
                let _ = status_tx.send(ProcessStatus::Failed { exit_code: -1 });
                return;
            }
        };
        pid.store(child_pid(&child), Ordering::SeqCst);
        *started_at.write().await = Utc::now();
        let _ = status_tx.send(ProcessStatus::Running);
    }
}

/// Send `sig` to the slot's current PID. Returns `Ok(true)` if the
/// signal landed, `Ok(false)` if the process was already gone or we
/// don't yet have a PID, `Err` for any other signal failure.
///
/// `nix::sys::signal::kill` on a non-positive pid is treacherous on
/// POSIX (negative values target process groups; zero targets the
/// caller's group). We refuse any pid `<= 0`.
pub(crate) fn signal_pid(
    pid_slot: &Arc<AtomicI32>,
    sig: nix::sys::signal::Signal,
) -> Result<bool, SupervisorError> {
    let pid = pid_slot.load(Ordering::SeqCst);
    if pid <= 0 {
        return Ok(false);
    }
    let target = nix::unistd::Pid::from_raw(pid);
    match nix::sys::signal::kill(target, sig) {
        Ok(()) => Ok(true),
        Err(nix::errno::Errno::ESRCH) => Ok(false),
        Err(e) => Err(SupervisorError::KillFailed {
            reason: format!("nix::kill({pid}, {sig:?}): {e}"),
        }),
    }
}

/// Best-effort fire-and-forget SIGKILL for `Supervisor::drop` to use.
/// Returns nothing; if signaling fails (ESRCH, EPERM) the synchronous
/// path can't do anything useful with the error.
// TODO(chum-v0.2): track per-process pid-generation tokens so we can
// distinguish "our child" from "some unrelated process that happens
// to have reused this pid in the microseconds since wait()."
pub(crate) fn force_kill_blocking(pid_slot: &Arc<AtomicI32>) {
    let pid = pid_slot.load(Ordering::SeqCst);
    if pid > 0 {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
}

/// Wait until `rx` reports a terminal status, with a hard `timeout`.
///
/// Returns `Ok(())` if a terminal status was observed in time,
/// `Err(SupervisorError::MonitorWedged)` if the sender dropped before
/// a terminal status, and `Err(SupervisorError::KillFailed)` with a
/// "timed out" reason if the timeout expired.
pub(crate) async fn wait_for_terminal(
    mut rx: watch::Receiver<ProcessStatus>,
    timeout: Duration,
    key: &ProcessKey,
) -> Result<(), SupervisorError> {
    let work = async {
        loop {
            if rx.borrow().is_terminal() {
                return Ok::<(), SupervisorError>(());
            }
            if rx.changed().await.is_err() {
                return Err(SupervisorError::MonitorWedged { key: key.clone() });
            }
        }
    };
    match tokio::time::timeout(timeout, work).await {
        Ok(r) => r,
        Err(_) => Err(SupervisorError::KillFailed {
            reason: format!("'{key}' did not reach terminal status within {timeout:?}"),
        }),
    }
}

fn child_pid(child: &Child) -> i32 {
    child.id().map(|id| id as i32).unwrap_or(-1)
}
