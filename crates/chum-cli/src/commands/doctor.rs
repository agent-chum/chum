//! `chum doctor` — runtime environment health-check. Validates the
//! v0.1 deployment story: toolchain, optional npm, CHUM_HOME
//! writability, chumd reachability, registry readability.
//!
//! Human output is a checklist of ✓ (pass) / ✗ (fail) / ⚠ (warn)
//! lines. `--json` emits a structured envelope for scripting.

use std::path::PathBuf;
use std::process::Command as StdCommand;

use clap::Args;

use crate::error::UserFacingError;

/// Arguments for `chum doctor`.
#[derive(Args, Debug)]
pub struct DoctorArgs {
    /// Override CHUM_HOME for the writability + registry checks.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Override the IPC socket path for the chumd-reachability check.
    #[arg(long)]
    pub socket_path: Option<PathBuf>,
    /// Emit machine-readable JSON instead of the human checklist.
    #[arg(long)]
    pub json: bool,
}

/// One check result.
#[derive(Debug, Clone)]
pub struct CheckResult {
    /// Human-readable check name.
    pub name: String,
    /// Outcome: ok / warn / fail.
    pub status: CheckStatus,
    /// One-line detail message rendered after the icon.
    pub message: String,
}

/// Three possible check outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    /// Check passed.
    Ok,
    /// Optional component missing or degraded; chum still works.
    Warn,
    /// Required component missing or broken; chum will not work.
    Fail,
}

impl CheckStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
    fn icon(&self, color: bool) -> &'static str {
        if !color {
            return match self {
                Self::Ok => "✓",
                Self::Warn => "⚠",
                Self::Fail => "✗",
            };
        }
        match self {
            Self::Ok => "\x1b[32m✓\x1b[0m",
            Self::Warn => "\x1b[33m⚠\x1b[0m",
            Self::Fail => "\x1b[31m✗\x1b[0m",
        }
    }
}

/// Execute `chum doctor`.
pub async fn run(args: DoctorArgs) -> Result<(), UserFacingError> {
    let mut checks = Vec::new();

    // 1. Rust toolchain (rustc presence implies the binary built).
    checks.push(check_rustc());

    // 2. npm (warning if missing — only required for npm-source manifests).
    checks.push(check_npm());

    // 3. CHUM_HOME writability.
    checks.push(check_chum_home(args.root.clone()));

    // 4. chumd reachability via IPC ping.
    checks.push(check_chumd(args.root.clone(), args.socket_path.clone()).await);

    // 5. Registry DB readability.
    checks.push(check_registry(args.root.clone()));

    let color = crate::term::color_enabled(false);

    if args.json {
        let entries: Vec<serde_json::Value> = checks
            .iter()
            .map(|c| {
                serde_json::json!({
                    "name": c.name,
                    "status": c.status.as_str(),
                    "message": c.message,
                })
            })
            .collect();
        let overall = if checks.iter().any(|c| c.status == CheckStatus::Fail) {
            "fail"
        } else if checks.iter().any(|c| c.status == CheckStatus::Warn) {
            "warn"
        } else {
            "ok"
        };
        let envelope = serde_json::json!({
            "status": "ok",
            "doctor": {
                "overall": overall,
                "checks": entries,
            }
        });
        println!("{envelope}");
    } else {
        println!("chum doctor — environment check");
        println!();
        let name_w = checks.iter().map(|c| c.name.len()).max().unwrap_or(0);
        for c in &checks {
            println!(
                "  {} {:nw$}  {}",
                c.status.icon(color),
                c.name,
                c.message,
                nw = name_w,
            );
        }
        println!();
        let failures = checks.iter().filter(|c| c.status == CheckStatus::Fail).count();
        if failures == 0 {
            println!("All required checks passed.");
        } else {
            println!("{failures} check(s) failed.");
        }
    }

    // chum doctor itself never errors — it returns 0 even when checks
    // fail, so scripts can rely on the exit code being driven by the
    // `overall` field. Use --json to programmatically branch.
    Ok(())
}

fn check_rustc() -> CheckResult {
    match StdCommand::new("rustc").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
            CheckResult {
                name: "rust toolchain".to_string(),
                status: CheckStatus::Ok,
                message: ver,
            }
        }
        Ok(_) => CheckResult {
            name: "rust toolchain".to_string(),
            status: CheckStatus::Warn,
            message: "rustc found but returned non-zero (build may fail)".to_string(),
        },
        Err(_) => CheckResult {
            name: "rust toolchain".to_string(),
            status: CheckStatus::Warn,
            message: "rustc not on PATH (only needed for source builds)".to_string(),
        },
    }
}

fn check_npm() -> CheckResult {
    match StdCommand::new("npm").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
            CheckResult {
                name: "npm".to_string(),
                status: CheckStatus::Ok,
                message: format!("npm {ver}"),
            }
        }
        _ => CheckResult {
            name: "npm".to_string(),
            status: CheckStatus::Warn,
            message: "not on PATH — needed only for npm-source manifests".to_string(),
        },
    }
}

fn check_chum_home(root_arg: Option<PathBuf>) -> CheckResult {
    let root = match crate::commands::resolve_root(root_arg) {
        Ok(r) => r,
        Err(_) => {
            return CheckResult {
                name: "CHUM_HOME".to_string(),
                status: CheckStatus::Fail,
                message: "could not resolve — set --root, $CHUM_HOME, $XDG_DATA_HOME, or $HOME".to_string(),
            };
        }
    };
    // mkdir -p then write a probe file.
    if std::fs::create_dir_all(&root).is_err() {
        return CheckResult {
            name: "CHUM_HOME".to_string(),
            status: CheckStatus::Fail,
            message: format!("cannot create {}", root.display()),
        };
    }
    let probe = root.join(".chum-doctor-probe");
    if let Err(e) = std::fs::write(&probe, b"probe") {
        return CheckResult {
            name: "CHUM_HOME".to_string(),
            status: CheckStatus::Fail,
            message: format!("cannot write to {}: {e}", root.display()),
        };
    }
    let _ = std::fs::remove_file(&probe);
    CheckResult {
        name: "CHUM_HOME".to_string(),
        status: CheckStatus::Ok,
        message: format!("writable at {}", root.display()),
    }
}

async fn check_chumd(
    root_arg: Option<PathBuf>,
    socket_arg: Option<PathBuf>,
) -> CheckResult {
    let root = match crate::commands::resolve_root(root_arg) {
        Ok(r) => r,
        Err(_) => {
            return CheckResult {
                name: "chumd IPC".to_string(),
                status: CheckStatus::Warn,
                message: "skipped — CHUM_HOME unresolved".to_string(),
            };
        }
    };
    let socket = socket_arg.unwrap_or_else(|| root.join("daemon.sock"));
    if !socket.exists() {
        return CheckResult {
            name: "chumd IPC".to_string(),
            status: CheckStatus::Warn,
            message: format!(
                "socket missing at {} (run: chumd & or chum daemon install-service)",
                socket.display()
            ),
        };
    }
    let client = chum_daemon::DaemonClient::new(socket.clone());
    match client.ping().await {
        Ok(p) => CheckResult {
            name: "chumd IPC".to_string(),
            status: CheckStatus::Ok,
            message: format!("daemon {} (uptime {}s)", p.daemon_version, p.uptime_secs),
        },
        Err(e) => CheckResult {
            name: "chumd IPC".to_string(),
            status: CheckStatus::Warn,
            message: format!("ping failed: {e}"),
        },
    }
}

fn check_registry(root_arg: Option<PathBuf>) -> CheckResult {
    let root = match crate::commands::resolve_root(root_arg) {
        Ok(r) => r,
        Err(_) => {
            return CheckResult {
                name: "registry".to_string(),
                status: CheckStatus::Warn,
                message: "skipped — CHUM_HOME unresolved".to_string(),
            };
        }
    };
    let db = root.join("state.db");
    if !db.is_file() {
        return CheckResult {
            name: "registry".to_string(),
            status: CheckStatus::Ok,
            message: format!("no state.db yet at {} (created on first install)", db.display()),
        };
    }
    match chum_registry::Registry::open(&db) {
        Ok(registry) => match registry.list_all() {
            Ok(rows) => CheckResult {
                name: "registry".to_string(),
                status: CheckStatus::Ok,
                message: format!("{} package(s) installed", rows.len()),
            },
            Err(e) => CheckResult {
                name: "registry".to_string(),
                status: CheckStatus::Fail,
                message: format!("state.db unreadable: {e}"),
            },
        },
        Err(e) => CheckResult {
            name: "registry".to_string(),
            status: CheckStatus::Fail,
            message: format!("state.db open failed: {e}"),
        },
    }
}
