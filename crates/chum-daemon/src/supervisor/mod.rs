//! In-memory process supervisor.
//!
//! [`Supervisor`] spawns and monitors child processes described by
//! a [`chum_install::InstalledArtifact`] + the originating
//! [`chum_core::Manifest`]. State is purely in-memory; the registry
//! persists what is *installed*, the supervisor tracks what is
//! *running*.
//!
//! v0.1 scope: spawn, monitor, restart, kill. No IPC, no launchd
//! integration, no MCP protocol awareness. Those land in subsequent
//! sessions.
//!
//! ## TODO markers
//!
// TODO(chum-v0.2): crash-loop detection — cap consecutive restarts
// inside some sliding window and surface `crash_looped` as a
// terminal status instead of restarting forever.
// TODO(chum-v0.2): persist supervisor state across daemon restarts
// (today the registry is the only persistent state; restart counts
// reset when the daemon does).

pub(crate) mod process;
pub mod restart;

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use chum_core::Manifest;
use chum_install::InstalledArtifact;
use tokio::sync::{RwLock, watch};

use crate::error::SupervisorError;
use crate::supervisor::process::{
    MonitorContext, ProcessSlot, SlotHandles, force_kill_blocking, monitor_loop, signal_pid,
    spawn_child, wait_for_terminal,
};
use crate::supervisor::restart::BackoffPolicy;

/// Identifier for a supervised process. Pairs the package name and
/// version that uniquely locate an installed artifact in the registry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProcessKey {
    /// Package name from the manifest.
    pub name: String,
    /// Package version from the manifest.
    pub version: String,
}

impl ProcessKey {
    /// Construct a key from a name and version.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }

    /// Build a key from an installed artifact.
    pub fn from_artifact(artifact: &InstalledArtifact) -> Self {
        Self::new(artifact.name.clone(), artifact.version.clone())
    }
}

impl fmt::Display for ProcessKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

/// Snapshot of a process at the moment a [`Supervisor`] method
/// returned. To observe current state, query via [`Supervisor::status`]
/// or [`Supervisor::list`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessHandle {
    /// OS pid at the time of return.
    pub pid: u32,
    /// UTC timestamp the most recent child was spawned at.
    pub started_at: DateTime<Utc>,
    /// Number of restarts that have happened so far (0 for a fresh
    /// process).
    pub restart_count: u32,
}

/// Lifecycle status of a supervised process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessStatus {
    /// The supervisor has called `spawn` but the child has not yet
    /// reported Running.
    Starting,
    /// The child is alive.
    Running,
    /// The previous child exited, restart policy says to restart,
    /// and the backoff sleep is in progress.
    Restarting,
    /// Terminal: the process exited cleanly (code 0) or was stopped
    /// by the supervisor.
    Stopped,
    /// Terminal: the process exited non-zero or was signal-terminated.
    Failed {
        /// Exit code reported by the OS, or `-1` for "killed by a
        /// signal / no code available."
        exit_code: i32,
    },
}

impl ProcessStatus {
    /// `true` for `Stopped` and `Failed` — the supervisor monitor
    /// has finished and the slot will not transition again until a
    /// fresh `spawn` or `restart`.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped | Self::Failed { .. })
    }
}

/// In-memory process supervisor.
///
/// `Supervisor` is `Send + Sync`. Multiple async tasks may hold a
/// clone (via `Arc<Supervisor>`) and call methods concurrently;
/// internal mutation is synchronized via a `std::sync::Mutex` on
/// the process map. The lock is held only for brief map ops and is
/// never held across `await`.
pub struct Supervisor {
    processes: Arc<std::sync::Mutex<HashMap<ProcessKey, ProcessSlot>>>,
    backoff: BackoffPolicy,
}

impl Supervisor {
    /// Construct a supervisor using the v0.1 standard
    /// [`BackoffPolicy::standard`] schedule (1s, 2s, 4s, 8s, 16s …).
    pub fn new() -> Self {
        Self::with_backoff(BackoffPolicy::standard())
    }

    /// Construct a supervisor with a custom backoff schedule.
    ///
    /// Tests typically use a fast schedule (e.g. `base = 50ms`,
    /// `cap = 200ms`) so `restart_count` assertions complete in
    /// under a second.
    pub fn with_backoff(backoff: BackoffPolicy) -> Self {
        Self {
            processes: Arc::new(std::sync::Mutex::new(HashMap::new())),
            backoff,
        }
    }

    /// Spawn a fresh child for `(artifact, manifest)`.
    ///
    /// # Errors
    /// - [`SupervisorError::AlreadyRunning`] if a slot with the same
    ///   `(name, version)` exists and is in a non-terminal status.
    ///   Stopped / Failed slots are replaced atomically.
    /// - [`SupervisorError::SpawnFailed`] if `Command::spawn` fails.
    pub async fn spawn(
        &self,
        artifact: InstalledArtifact,
        manifest: Manifest,
    ) -> Result<ProcessHandle, SupervisorError> {
        let key = ProcessKey::from_artifact(&artifact);

        // Fast pre-check: refuse duplicate spawn on a live slot.
        {
            let map = self.processes.lock().expect("supervisor map poisoned");
            if let Some(existing) = map.get(&key) {
                if !existing.status_rx.borrow().is_terminal() {
                    return Err(SupervisorError::AlreadyRunning { key });
                }
            }
        }

        let child = spawn_child(&artifact, &manifest)?;
        let pid = child.id().map(|id| id as i32).unwrap_or(-1);
        let started_at = Utc::now();

        let pid_slot = Arc::new(AtomicI32::new(pid));
        let started_at_slot = Arc::new(RwLock::new(started_at));
        let shutdown = Arc::new(AtomicBool::new(false));
        let restart_count = Arc::new(AtomicU32::new(0));
        let (status_tx, status_rx) = watch::channel(ProcessStatus::Starting);

        let ctx = MonitorContext {
            key: key.clone(),
            artifact: artifact.clone(),
            manifest: manifest.clone(),
            pid: pid_slot.clone(),
            started_at: started_at_slot.clone(),
            shutdown: shutdown.clone(),
            restart_count: restart_count.clone(),
            status_tx: status_tx.clone(),
            backoff: self.backoff,
        };
        let monitor = tokio::spawn(monitor_loop(ctx, child));

        let slot = ProcessSlot {
            artifact,
            manifest,
            pid: pid_slot,
            shutdown,
            restart_count: restart_count.clone(),
            status_rx,
            monitor,
        };
        // status_tx + started_at_slot remain in the closures captured
        // by the monitor task; they are not needed in the supervisor map.
        let _ = (status_tx, started_at_slot);

        // Replace any prior terminal slot; abort its (already-finished)
        // monitor handle so we don't leak the join.
        {
            let mut map = self.processes.lock().expect("supervisor map poisoned");
            if let Some(prior) = map.remove(&key) {
                prior.monitor.abort();
            }
            map.insert(key.clone(), slot);
        }

        Ok(ProcessHandle {
            pid: pid.max(0) as u32,
            started_at,
            restart_count: restart_count.load(Ordering::SeqCst),
        })
    }

    /// Stop a running process gracefully: SIGTERM, wait up to
    /// `grace`, then SIGKILL if still alive.
    ///
    /// # Errors
    /// - [`SupervisorError::NotRunning`] if no slot exists.
    /// - [`SupervisorError::KillFailed`] if signal delivery fails or
    ///   the child survives SIGKILL for longer than the
    ///   `SIGKILL_HARD_CEILING` window.
    /// - [`SupervisorError::MonitorWedged`] if the monitor task
    ///   ended without writing a terminal status.
    pub async fn stop(&self, key: &ProcessKey, grace: Duration) -> Result<(), SupervisorError> {
        let handles = self
            .handles_for(key)
            .ok_or_else(|| SupervisorError::NotRunning { key: key.clone() })?;

        // Already-terminal slot: nothing to do.
        if handles.status_rx.borrow().is_terminal() {
            return Ok(());
        }

        handles.shutdown.store(true, Ordering::SeqCst);
        let _ = signal_pid(&handles.pid, nix::sys::signal::Signal::SIGTERM)?;

        match wait_for_terminal(handles.status_rx.clone(), grace, key).await {
            Ok(()) => Ok(()),
            Err(SupervisorError::KillFailed { .. }) => {
                // SIGTERM grace expired — escalate.
                let _ = signal_pid(&handles.pid, nix::sys::signal::Signal::SIGKILL)?;
                wait_for_terminal(handles.status_rx, SIGKILL_HARD_CEILING, key).await
            }
            Err(e) => Err(e),
        }
    }

    /// Stop a running process with an immediate SIGKILL.
    pub async fn kill(&self, key: &ProcessKey) -> Result<(), SupervisorError> {
        let handles = self
            .handles_for(key)
            .ok_or_else(|| SupervisorError::NotRunning { key: key.clone() })?;

        if handles.status_rx.borrow().is_terminal() {
            return Ok(());
        }

        handles.shutdown.store(true, Ordering::SeqCst);
        let _ = signal_pid(&handles.pid, nix::sys::signal::Signal::SIGKILL)?;
        wait_for_terminal(handles.status_rx, SIGKILL_HARD_CEILING, key).await
    }

    /// Stop a running process (with a 5s grace) and re-spawn from
    /// the same artifact + manifest.
    pub async fn restart(&self, key: &ProcessKey) -> Result<ProcessHandle, SupervisorError> {
        let handles = self
            .handles_for(key)
            .ok_or_else(|| SupervisorError::NotRunning { key: key.clone() })?;

        if !handles.status_rx.borrow().is_terminal() {
            self.stop(key, Duration::from_secs(5)).await?;
        }

        self.spawn(handles.artifact, handles.manifest).await
    }

    /// Current status for `key`, or `None` if no slot exists.
    pub async fn status(&self, key: &ProcessKey) -> Option<ProcessStatus> {
        let map = self.processes.lock().ok()?;
        map.get(key).map(|s| s.status_rx.borrow().clone())
    }

    /// Snapshot of every registered slot. Order is implementation-
    /// defined (HashMap iteration); callers wanting a stable order
    /// should sort by key.
    pub async fn list(&self) -> Vec<(ProcessKey, ProcessStatus)> {
        let map = match self.processes.lock() {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };
        map.iter()
            .map(|(k, s)| (k.clone(), s.status_rx.borrow().clone()))
            .collect()
    }

    /// Restart-count snapshot for `key`, or `None` if no slot. Useful
    /// for tests asserting monotonic increment under
    /// [`chum_core::manifest::RestartPolicy::Always`].
    pub async fn restart_count(&self, key: &ProcessKey) -> Option<u32> {
        let map = self.processes.lock().ok()?;
        map.get(key)
            .map(|s| s.restart_count.load(Ordering::SeqCst))
    }

    /// Current OS pid for `key`, or `None` if no slot exists or the
    /// slot is between waits (status `Restarting`). The lookup is
    /// async only to match the rest of the API surface — internally
    /// it touches a single atomic load.
    pub async fn pid(&self, key: &ProcessKey) -> Option<u32> {
        let map = self.processes.lock().ok()?;
        let slot = map.get(key)?;
        let pid = slot.pid.load(Ordering::SeqCst);
        if pid > 0 { Some(pid as u32) } else { None }
    }

    fn handles_for(&self, key: &ProcessKey) -> Option<SlotHandles> {
        let map = self.processes.lock().ok()?;
        map.get(key).map(|s| s.handles())
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Supervisor {
    /// Best-effort teardown: send `SIGKILL` to every live PID and
    /// `.abort()` every monitor task. Synchronous because `Drop`
    /// cannot `await`. Children that have already exited yield
    /// `ESRCH` from `nix::kill` which we silently swallow.
    fn drop(&mut self) {
        if let Ok(map) = self.processes.lock() {
            for slot in map.values() {
                force_kill_blocking(&slot.pid);
                slot.monitor.abort();
            }
        }
    }
}

/// Hard ceiling for "child still alive after SIGKILL." If the kernel
/// hasn't delivered the signal in this long, the supervisor returns
/// [`SupervisorError::KillFailed`] rather than hang forever.
const SIGKILL_HARD_CEILING: Duration = Duration::from_secs(2);
