//! User-facing success output. Pairs with [`crate::error::render`] for
//! failures.
//!
//! Two modes:
//!
//! - **Human** — single-line `Installed <name> <version> at <dir>` on
//!   stdout. The default.
//! - **JSON** — a stable envelope on stdout consumed by scripts.
//!   Shape is locked at v0.1 and documented inline below.

use std::path::Path;

use chum_core::Manifest;
use chum_daemon::{
    PingResponse, ProcessStatusResponse, RestartProcessResponse, SpawnResponse, StatusResponse,
    TailLogsResponse, TerminateResponse,
};
use chum_install::{InstalledArtifact, SourceKind};
use chum_registry::RegistryArtifact;

/// Stable string form for [`SourceKind`] used in JSON output and
/// shared with the registry's column encoding.
fn source_kind_str(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Npm => "npm",
        SourceKind::Local => "local",
        SourceKind::Binary => "binary",
        _ => "unknown",
    }
}

/// Render `install_dir` relative to the CHUM root for compact column
/// output. Falls back to the absolute path if `install_dir` somehow
/// lives outside `root` (shouldn't happen for registry-written rows).
fn relative_to_root<'a>(install_dir: &'a Path, root: &Path) -> std::path::PathBuf {
    install_dir
        .strip_prefix(root)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| install_dir.to_path_buf())
}

/// Hint emitted after a successful `chum install` when the manifest
/// declares permissions. Tells the user exactly which
/// `chum permit --grant <kind>=<value>` calls they'll need to run
/// before `chum start` works.
///
/// JSON mode skips this — the structured envelope already includes
/// enough info via the manifest data the caller could read.
pub fn emit_install_permission_hint(manifest: &Manifest, json: bool) {
    if json {
        return;
    }
    if manifest.permissions.is_empty() {
        return;
    }
    println!();
    println!(
        "This manifest declares permissions you'll need to grant before 'chum start':"
    );
    for req in manifest.permissions.iter_requirements() {
        println!(
            "    chum permit {} --grant {}={}",
            manifest.package.name, req.kind, req.value,
        );
    }
}

/// Print confirmation that a manifest was installed and persisted.
///
/// JSON envelope:
/// ```json
/// {
///   "status": "ok",
///   "installed": {
///     "name": "...",
///     "version": "...",
///     "install_dir": "...",
///     "entrypoint": "...",
///     "source_kind": "npm|local|binary"
///   }
/// }
/// ```
///
/// The registry-assigned `id` is deliberately omitted — it is an
/// internal detail. Everything a script needs to chain a subsequent
/// `chum uninstall <name>` or `chum list` is here.
pub fn emit_installed(artifact: &InstalledArtifact, json: bool) {
    if json {
        let envelope = serde_json::json!({
            "status": "ok",
            "installed": {
                "name": artifact.name,
                "version": artifact.version,
                "install_dir": artifact.install_dir.display().to_string(),
                "entrypoint": artifact.entrypoint.display().to_string(),
                "source_kind": source_kind_str(artifact.source_kind),
            }
        });
        println!("{envelope}");
    } else {
        println!(
            "Installed {} {} at {}",
            artifact.name,
            artifact.version,
            artifact.install_dir.display()
        );
    }
}

/// Confirmation that a package was uninstalled.
///
/// JSON envelope:
/// ```json
/// {
///   "status": "ok",
///   "uninstalled": {
///     "name": "...",
///     "version": "...",
///     "keep_files": false
///   }
/// }
/// ```
///
/// Human form: `Uninstalled <name> <version>` plus, when
/// `keep_files` is true, a trailing note that the files were
/// retained on disk.
pub fn emit_uninstalled(target: &RegistryArtifact, keep_files: bool, json: bool) {
    if json {
        let envelope = serde_json::json!({
            "status": "ok",
            "uninstalled": {
                "name": target.name,
                "version": target.version,
                "keep_files": keep_files,
            }
        });
        println!("{envelope}");
    } else if keep_files {
        println!(
            "Uninstalled {} {} (files retained at {})",
            target.name,
            target.version,
            target.install_dir.display(),
        );
    } else {
        println!("Uninstalled {} {}", target.name, target.version);
    }
}

/// User answered "no" at the y/N prompt — emit a cancelled envelope
/// and let `main` exit 0. Cancellation is not an error.
///
/// JSON envelope:
/// ```json
/// { "status": "cancelled", "name": "...", "version": "..." }
/// ```
pub fn emit_uninstall_cancelled(target: &RegistryArtifact, json: bool) {
    if json {
        let envelope = serde_json::json!({
            "status": "cancelled",
            "name": target.name,
            "version": target.version,
        });
        println!("{envelope}");
    } else {
        println!("Uninstall cancelled.");
    }
}

/// `chum start` confirmation. JSON envelope:
/// `{"status":"ok","started":{"name","version","pid","started_at"}}`.
pub fn emit_started(name: &str, version: &str, resp: &SpawnResponse, json: bool) {
    if json {
        let envelope = serde_json::json!({
            "status": "ok",
            "started": {
                "name": name,
                "version": version,
                "pid": resp.pid,
                "started_at": resp.started_at,
            }
        });
        println!("{envelope}");
    } else {
        println!(
            "Started {} {} (pid {}, at {})",
            name, version, resp.pid, resp.started_at,
        );
    }
}

/// `chum stop` confirmation. JSON envelope:
/// `{"status":"ok","stopped":{"name","version"}}`.
pub fn emit_stopped(name: &str, version: &str, _resp: &TerminateResponse, json: bool) {
    if json {
        let envelope = serde_json::json!({
            "status": "ok",
            "stopped": {
                "name": name,
                "version": version,
            }
        });
        println!("{envelope}");
    } else {
        println!("Stopped {} {}", name, version);
    }
}

/// `chum restart` confirmation. JSON envelope mirrors `emit_started`
/// with an added `restart_count`.
pub fn emit_restarted(
    name: &str,
    version: &str,
    resp: &RestartProcessResponse,
    json: bool,
) {
    if json {
        let envelope = serde_json::json!({
            "status": "ok",
            "restarted": {
                "name": name,
                "version": version,
                "pid": resp.pid,
                "started_at": resp.started_at,
                "restart_count": resp.restart_count,
            }
        });
        println!("{envelope}");
    } else {
        println!(
            "Restarted {} {} (pid {}, restart_count {})",
            name, version, resp.pid, resp.restart_count,
        );
    }
}

/// `chum env list` output. Keys only — values are never shown,
/// even in JSON mode (the manifest's `runtime.env` may carry
/// secrets).
pub fn emit_env_list(
    name: &str,
    version: &str,
    entries: &[(String, bool)],
    json: bool,
) {
    if json {
        let entries_json: Vec<serde_json::Value> = entries
            .iter()
            .map(|(k, set)| {
                serde_json::json!({"key": k, "status": if *set { "set" } else { "unset" }})
            })
            .collect();
        let envelope = serde_json::json!({
            "status": "ok",
            "env": {
                "name": name,
                "version": version,
                "entries": entries_json,
            }
        });
        println!("{envelope}");
        return;
    }
    println!("chum env for {name} {version}");
    println!();
    if entries.is_empty() {
        println!("  (no env keys declared or set)");
        return;
    }
    let key_width = entries
        .iter()
        .map(|(k, _)| k.len())
        .max()
        .unwrap_or(0)
        .max("KEY".len());
    println!("  {:kw$}  STATUS", "KEY", kw = key_width);
    for (key, set) in entries {
        let status = if *set { "set" } else { "unset" };
        println!("  {:kw$}  {}", key, status, kw = key_width);
    }
}

/// `chum env set` confirmation. The value is never echoed.
pub fn emit_env_set(name: &str, version: &str, key: &str, json: bool) {
    if json {
        let envelope = serde_json::json!({
            "status": "ok",
            "env_set": {"name": name, "version": version, "key": key}
        });
        println!("{envelope}");
    } else {
        println!("Set {key} in {name} {version} (value not echoed)");
        println!("Run 'chum restart {name}' for the change to take effect.");
    }
}

/// `chum env unset` confirmation. `was_set = false` is idempotent
/// success.
pub fn emit_env_unset(name: &str, version: &str, key: &str, was_set: bool, json: bool) {
    if json {
        let envelope = serde_json::json!({
            "status": "ok",
            "env_unset": {
                "name": name,
                "version": version,
                "key": key,
                "was_set": was_set,
            }
        });
        println!("{envelope}");
    } else if was_set {
        println!("Unset {key} in {name} {version}");
        println!("Run 'chum restart {name}' for the change to take effect.");
    } else {
        println!("{key} was not set in {name} {version} (no-op)");
    }
}

/// `chum permit` confirmation listing every grant that landed.
pub fn emit_granted(
    name: &str,
    version: &str,
    granted: &[(String, String)],
    json: bool,
) {
    if json {
        let entries: Vec<serde_json::Value> = granted
            .iter()
            .map(|(k, v)| serde_json::json!({"kind": k, "value": v}))
            .collect();
        let envelope = serde_json::json!({
            "status": "ok",
            "granted": {
                "name": name,
                "version": version,
                "grants": entries,
            }
        });
        println!("{envelope}");
    } else {
        println!("Granted to {name} {version}:");
        for (k, v) in granted {
            println!("  {k}={v}");
        }
    }
}

/// `chum revoke` confirmation.
pub fn emit_revoked(name: &str, version: &str, kind: &str, value: &str, json: bool) {
    if json {
        let envelope = serde_json::json!({
            "status": "ok",
            "revoked": {
                "name": name,
                "version": version,
                "kind": kind,
                "value": value,
            }
        });
        println!("{envelope}");
    } else {
        println!("Revoked from {name} {version}: {kind}={value}");
    }
}

/// `chum permissions` three-section diff.
pub fn emit_permissions(
    name: &str,
    version: &str,
    declared: &[(String, String)],
    granted: &[chum_registry::Grant],
    missing: &[(String, String)],
    json: bool,
) {
    if json {
        let declared_json: Vec<serde_json::Value> = declared
            .iter()
            .map(|(k, v)| serde_json::json!({"kind": k, "value": v}))
            .collect();
        let granted_json: Vec<serde_json::Value> = granted
            .iter()
            .map(|g| {
                serde_json::json!({
                    "kind": g.kind,
                    "value": g.value,
                    "granted_at": g.granted_at.to_rfc3339(),
                })
            })
            .collect();
        let missing_json: Vec<serde_json::Value> = missing
            .iter()
            .map(|(k, v)| serde_json::json!({"kind": k, "value": v}))
            .collect();
        let envelope = serde_json::json!({
            "status": "ok",
            "permissions": {
                "name": name,
                "version": version,
                "declared": declared_json,
                "granted": granted_json,
                "missing": missing_json,
            }
        });
        println!("{envelope}");
        return;
    }
    println!("{name} {version}");
    println!();
    println!("Declared by manifest:");
    if declared.is_empty() {
        println!("  (none)");
    } else {
        for (k, v) in declared {
            println!("  {k}  {v}");
        }
    }
    println!();
    println!("Granted:");
    if granted.is_empty() {
        println!("  (none)");
    } else {
        for g in granted {
            println!("  {}  {}", g.kind, g.value);
        }
    }
    println!();
    println!("Missing (would block 'chum start'):");
    if missing.is_empty() {
        println!("  (none)");
    } else {
        for (k, v) in missing {
            println!("  {k}  {v}");
        }
    }
}

/// `chum daemon install-service` confirmation. When `no_load` is
/// true (the test path), the message reflects that.
pub fn emit_service_installed(
    cfg: &crate::commands::daemon_service::ServiceConfig,
    no_load: bool,
    json: bool,
) {
    if json {
        let envelope = serde_json::json!({
            "status": "ok",
            "service_installed": {
                "label": crate::commands::daemon_service::LAUNCHD_LABEL,
                "plist_path": cfg.plist_path.display().to_string(),
                "chumd_path": cfg.chumd_path.display().to_string(),
                "chum_home": cfg.chum_home.display().to_string(),
                "loaded": !no_load,
            }
        });
        println!("{envelope}");
    } else {
        println!(
            "LaunchAgent installed at {} ({})",
            cfg.plist_path.display(),
            if no_load {
                "plist written, launchctl load skipped"
            } else {
                "loaded via launchctl"
            },
        );
    }
}

/// `chum daemon uninstall-service` confirmation. `removed` is `false`
/// when no plist existed to begin with (still success — idempotent).
pub fn emit_service_uninstalled(
    plist_path: &std::path::Path,
    removed: bool,
    json: bool,
) {
    if json {
        let envelope = serde_json::json!({
            "status": "ok",
            "service_uninstalled": {
                "label": crate::commands::daemon_service::LAUNCHD_LABEL,
                "plist_path": plist_path.display().to_string(),
                "removed": removed,
            }
        });
        println!("{envelope}");
    } else if removed {
        println!("LaunchAgent uninstalled ({} removed)", plist_path.display());
    } else {
        println!(
            "LaunchAgent already absent ({} did not exist)",
            plist_path.display(),
        );
    }
}

/// `chum daemon service-status` output.
pub fn emit_service_status(
    status: &crate::commands::daemon_service::ServiceStatus,
    json: bool,
) {
    if json {
        let envelope = serde_json::json!({
            "status": "ok",
            "service_status": {
                "label": crate::commands::daemon_service::LAUNCHD_LABEL,
                "loaded": status.loaded,
                "pid": status.pid,
                "last_exit_status": status.last_exit_status,
            }
        });
        println!("{envelope}");
    } else if !status.loaded {
        println!(
            "LaunchAgent '{}' is not loaded",
            crate::commands::daemon_service::LAUNCHD_LABEL,
        );
    } else {
        println!(
            "LaunchAgent '{}'",
            crate::commands::daemon_service::LAUNCHD_LABEL,
        );
        match status.pid {
            Some(p) => println!("  pid:               {p}"),
            None => println!("  pid:               (not running)"),
        }
        match status.last_exit_status {
            Some(c) => println!("  last_exit_status:  {c}"),
            None => println!("  last_exit_status:  (none)"),
        }
    }
}

/// `chum logs` output. In human mode prints the daemon's `content`
/// field directly to stdout (no decoration — it's already shaped for
/// human consumption, including section headers when stream is
/// `both`). JSON mode wraps the response in a stable envelope.
pub fn emit_logs(resp: &TailLogsResponse, json: bool) {
    if json {
        let envelope = serde_json::json!({
            "status": "ok",
            "logs": {
                "stream": resp.stream,
                "content": resp.content,
            }
        });
        println!("{envelope}");
    } else {
        println!("{}", resp.content);
    }
}

/// `chum status` output for a single process. JSON envelope:
/// `{"status":"ok","process":{...}}`.
pub fn emit_process_status(resp: &ProcessStatusResponse, json: bool) {
    if json {
        let mut process = serde_json::json!({
            "name": resp.name,
            "version": resp.version,
            "status": resp.status,
            "restart_count": resp.restart_count,
        });
        if let Some(p) = resp.pid {
            process["pid"] = serde_json::json!(p);
        }
        if let Some(c) = resp.exit_code {
            process["exit_code"] = serde_json::json!(c);
        }
        let envelope = serde_json::json!({
            "status": "ok",
            "process": process,
        });
        println!("{envelope}");
    } else {
        println!("{} {}", resp.name, resp.version);
        println!("  status:        {}", resp.status);
        if let Some(p) = resp.pid {
            println!("  pid:           {p}");
        }
        println!("  restart_count: {}", resp.restart_count);
        if let Some(c) = resp.exit_code {
            println!("  exit_code:     {c}");
        }
    }
}

/// Confirmation that the daemon answered a `ping`.
///
/// JSON envelope:
/// ```json
/// {
///   "status": "ok",
///   "daemon": {
///     "version": "0.1.0",
///     "uptime_secs": 42,
///     "installed_count": 3
///   }
/// }
/// ```
///
/// Human form: `chumd ok (uptime Xs, N installed)`.
pub fn emit_daemon_ping(ping: &PingResponse, json: bool) {
    if json {
        let envelope = serde_json::json!({
            "status": "ok",
            "daemon": {
                "version": ping.daemon_version,
                "uptime_secs": ping.uptime_secs,
                "installed_count": ping.installed_count,
            }
        });
        println!("{envelope}");
    } else {
        println!(
            "chumd ok (uptime {}s, {} installed)",
            ping.uptime_secs, ping.installed_count,
        );
    }
}

/// Render the daemon's `status` envelope.
///
/// JSON envelope:
/// ```json
/// {
///   "status": "ok",
///   "daemon": {
///     "pid": 12345,
///     "started_at": "2026-05-21T13:30:00+00:00",
///     "installed_count": 3,
///     "running_count": 0
///   }
/// }
/// ```
///
/// Human form: a small key/value table on stdout.
pub fn emit_daemon_status(status: &StatusResponse, json: bool) {
    if json {
        let envelope = serde_json::json!({
            "status": "ok",
            "daemon": {
                "pid": status.pid,
                "started_at": status.started_at,
                "installed_count": status.installed_count,
                "running_count": status.running_count,
            }
        });
        println!("{envelope}");
    } else {
        println!("chumd status");
        println!("  pid:              {}", status.pid);
        println!("  started_at:       {}", status.started_at);
        println!("  installed_count:  {}", status.installed_count);
        println!("  running_count:    {}", status.running_count);
    }
}

/// Render the result of `chum list`.
///
/// JSON envelope:
/// ```json
/// {
///   "status": "ok",
///   "packages": [
///     {
///       "name": "...",
///       "version": "...",
///       "source_kind": "npm|local|binary",
///       "install_dir": "...",
///       "installed_at": "2026-05-21T13:30:00+00:00"
///     }
///   ]
/// }
/// ```
///
/// Human form: a fixed-width table with columns
/// `NAME | VERSION | KIND | INSTALLED | PATH`, where `PATH` is
/// rendered relative to `root` for compactness. An empty list prints
/// `No packages installed.` and exits 0.
pub fn emit_list(rows: &[RegistryArtifact], root: &Path, json: bool) {
    if json {
        let packages: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "version": r.version,
                    "source_kind": source_kind_str(r.source_kind),
                    "install_dir": r.install_dir.display().to_string(),
                    "installed_at": r.installed_at.to_rfc3339(),
                })
            })
            .collect();
        let envelope = serde_json::json!({
            "status": "ok",
            "packages": packages,
        });
        println!("{envelope}");
        return;
    }

    if rows.is_empty() {
        println!("No packages installed.");
        return;
    }

    let installed_fmt: Vec<String> = rows
        .iter()
        .map(|r| r.installed_at.format("%Y-%m-%d %H:%M").to_string())
        .collect();
    let paths: Vec<String> = rows
        .iter()
        .map(|r| relative_to_root(&r.install_dir, root).display().to_string())
        .collect();

    let name_w = rows
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(0)
        .max("NAME".len());
    let ver_w = rows
        .iter()
        .map(|r| r.version.len())
        .max()
        .unwrap_or(0)
        .max("VERSION".len());
    let kind_w = rows
        .iter()
        .map(|r| source_kind_str(r.source_kind).len())
        .max()
        .unwrap_or(0)
        .max("KIND".len());
    let installed_w = installed_fmt
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(0)
        .max("INSTALLED".len());

    println!(
        "{:nw$}  {:vw$}  {:kw$}  {:iw$}  PATH",
        "NAME",
        "VERSION",
        "KIND",
        "INSTALLED",
        nw = name_w,
        vw = ver_w,
        kw = kind_w,
        iw = installed_w,
    );
    for (i, r) in rows.iter().enumerate() {
        println!(
            "{:nw$}  {:vw$}  {:kw$}  {:iw$}  {}",
            r.name,
            r.version,
            source_kind_str(r.source_kind),
            installed_fmt[i],
            paths[i],
            nw = name_w,
            vw = ver_w,
            kw = kind_w,
            iw = installed_w,
        );
    }
}

/// Print what `chum install` would do without touching the disk.
///
/// JSON envelope:
/// ```json
/// {
///   "status": "dry-run",
///   "manifest": { "name": "...", "version": "..." },
///   "root": "...",
///   "would_install_at": "<root>/packages/<name>/<version>"
/// }
/// ```
///
/// Echoing the resolved `root` lets the caller verify that a
/// `--root` override was honored.
pub fn emit_dry_run(manifest: &Manifest, root: &Path, json: bool) {
    let would_install_at = root
        .join("packages")
        .join(&manifest.package.name)
        .join(&manifest.package.version);

    if json {
        let envelope = serde_json::json!({
            "status": "dry-run",
            "manifest": {
                "name": manifest.package.name,
                "version": manifest.package.version,
            },
            "root": root.display().to_string(),
            "would_install_at": would_install_at.display().to_string(),
        });
        println!("{envelope}");
    } else {
        println!(
            "Dry run: would install {} {} under {} (target: {})",
            manifest.package.name,
            manifest.package.version,
            root.display(),
            would_install_at.display(),
        );
    }
}
