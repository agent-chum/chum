//! `chum daemon install-service / uninstall-service / service-status`
//! — manage the macOS LaunchAgent that keeps `chumd` running across
//! reboots.
//!
//! v0.1 macOS-only. LaunchAgents run only when the user is logged in
//! (FileVault-encrypted volumes mount on login, not at boot — this
//! is the same tradeoff Karoshi's other LaunchAgents accept).
//!
//! No plist crate dependency: the .plist file is rendered from a
//! `format!`-based template and `launchctl list` output is line-
//! scanned for `"PID"` / `"LastExitStatus"` keys (it's OpenStep
//! format, not XML, so a real plist parser would be overkill).

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use clap::Args;

use crate::error::UserFacingError;
use crate::output;

/// LaunchAgent label used everywhere: filename, launchctl handle.
pub const LAUNCHD_LABEL: &str = "cloud.chum.daemon";

/// Arguments for `chum daemon install-service`.
#[derive(Args, Debug)]
pub struct InstallServiceArgs {
    /// Override the path to the `chumd` binary baked into the plist.
    /// Default: the `chumd` binary alongside this `chum` binary.
    #[arg(long)]
    pub chumd_path: Option<PathBuf>,

    /// Override CHUM_HOME baked into the plist. Default: the resolved
    /// `chum_home()` at install-service time.
    #[arg(long)]
    pub root: Option<PathBuf>,

    /// Replace an existing plist + reload the LaunchAgent. Without
    /// `--force`, install-service refuses if the plist file already
    /// exists.
    #[arg(long)]
    pub force: bool,

    /// Override the `~/Library/LaunchAgents/` directory. Tests pass
    /// a tempdir here; users virtually never need this flag.
    #[arg(long)]
    pub plist_dir: Option<PathBuf>,

    /// Override the `~/Library/Logs/` directory baked into the plist.
    #[arg(long)]
    pub log_dir: Option<PathBuf>,

    /// Skip the `launchctl load` step. Used by the integration test
    /// to verify plist generation without actually loading anything.
    #[arg(long)]
    pub no_load: bool,

    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `chum daemon uninstall-service`.
#[derive(Args, Debug)]
pub struct UninstallServiceArgs {
    /// Override the `~/Library/LaunchAgents/` directory.
    #[arg(long)]
    pub plist_dir: Option<PathBuf>,

    /// Skip the `launchctl unload` step. Used by the integration test.
    #[arg(long)]
    pub no_unload: bool,

    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `chum daemon service-status`.
#[derive(Args, Debug)]
pub struct ServiceStatusArgs {
    /// Override the `launchctl` binary path. Tests can shim a fake.
    #[arg(long)]
    pub launchctl_path: Option<PathBuf>,

    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

/// Resolved configuration for the LaunchAgent.
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// Absolute path to the `chumd` binary baked into the plist.
    pub chumd_path: PathBuf,
    /// Absolute CHUM_HOME baked into the plist.
    pub chum_home: PathBuf,
    /// Absolute path the plist file will be written to.
    pub plist_path: PathBuf,
    /// Absolute path the LaunchAgent will redirect stdout into.
    pub stdout_log: PathBuf,
    /// Absolute path the LaunchAgent will redirect stderr into.
    pub stderr_log: PathBuf,
    /// `PATH` env value baked into the plist's
    /// `EnvironmentVariables` dict.
    pub path_env: String,
}

impl ServiceConfig {
    /// Resolve a config from optional overrides, falling back to
    /// `chum_home()` for the root and `current_exe`'s sibling for
    /// the chumd binary.
    ///
    /// IMPORTANT (verified by Karoshi's prior macOS work): env vars
    /// are baked directly into the plist via the
    /// `EnvironmentVariables` dict — `launchctl setenv` does not
    /// propagate reliably for LaunchAgents and is not used here.
    pub fn resolve(
        chumd_arg: Option<PathBuf>,
        root_arg: Option<PathBuf>,
        plist_dir_arg: Option<PathBuf>,
        log_dir_arg: Option<PathBuf>,
    ) -> Result<Self, UserFacingError> {
        let chumd_path = match chumd_arg {
            Some(p) => p,
            None => default_chumd_path()?,
        };
        let chum_home = crate::commands::resolve_root(root_arg)?;
        let plist_dir = match plist_dir_arg {
            Some(p) => p,
            None => default_launch_agents_dir()?,
        };
        let log_dir = match log_dir_arg {
            Some(p) => p,
            None => default_log_dir()?,
        };
        let plist_path = plist_dir.join(format!("{LAUNCHD_LABEL}.plist"));
        let stdout_log = log_dir.join("chum-daemon.stdout.log");
        let stderr_log = log_dir.join("chum-daemon.stderr.log");
        let path_env = std::env::var("PATH")
            .unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".to_string());

        Ok(Self {
            chumd_path,
            chum_home,
            plist_path,
            stdout_log,
            stderr_log,
            path_env,
        })
    }

    /// Render the launchd plist file content as a UTF-8 string.
    ///
    /// XML escapes the four characters that matter for the values we
    /// substitute in (`<`, `>`, `&`, `"`). Paths and PATH env are
    /// the only user-supplied fields; the static template strings
    /// are safe.
    pub fn render_plist(&self) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{chumd}</string>
        <string>--root</string>
        <string>{chum_home}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>{path_env}</string>
        <key>CHUM_HOME</key>
        <string>{chum_home}</string>
    </dict>
    <key>StandardOutPath</key>
    <string>{stdout_log}</string>
    <key>StandardErrorPath</key>
    <string>{stderr_log}</string>
    <key>WorkingDirectory</key>
    <string>{chum_home}</string>
</dict>
</plist>
"#,
            label = LAUNCHD_LABEL,
            chumd = xml_escape(&self.chumd_path.display().to_string()),
            chum_home = xml_escape(&self.chum_home.display().to_string()),
            path_env = xml_escape(&self.path_env),
            stdout_log = xml_escape(&self.stdout_log.display().to_string()),
            stderr_log = xml_escape(&self.stderr_log.display().to_string()),
        )
    }
}

/// Status parsed from `launchctl list <label>`.
#[derive(Debug, Clone)]
pub struct ServiceStatus {
    /// Whether the LaunchAgent is loaded (the label is known to
    /// launchctl).
    pub loaded: bool,
    /// OS pid if the daemon is currently running, else `None`.
    pub pid: Option<i32>,
    /// Last exit status reported by launchctl, if any.
    pub last_exit_status: Option<i32>,
}

/// Execute `chum daemon install-service`.
pub async fn run_install_service(args: InstallServiceArgs) -> Result<(), UserFacingError> {
    let cfg = ServiceConfig::resolve(
        args.chumd_path,
        args.root,
        args.plist_dir,
        args.log_dir,
    )?;

    if cfg.plist_path.exists() && !args.force {
        return Err(UserFacingError::ServiceAlreadyInstalled {
            path: cfg.plist_path.clone(),
        });
    }

    // Best-effort unload before overwriting (only on --force).
    if cfg.plist_path.exists() && args.force && !args.no_load {
        let _ = launchctl_unload(&cfg.plist_path);
    }

    if let Some(parent) = cfg.plist_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| UserFacingError::RootIo {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let body = cfg.render_plist();
    std::fs::write(&cfg.plist_path, body).map_err(|source| UserFacingError::RootIo {
        path: cfg.plist_path.clone(),
        source,
    })?;

    set_mode_644(&cfg.plist_path);

    if !args.no_load {
        launchctl_load(&cfg.plist_path).map_err(|reason| {
            UserFacingError::ServiceCommandFailed {
                cmd: format!("launchctl load {}", cfg.plist_path.display()),
                stderr: reason,
            }
        })?;
    }

    output::emit_service_installed(&cfg, args.no_load, args.json);
    Ok(())
}

/// Execute `chum daemon uninstall-service`. Idempotent — a missing
/// plist file (already removed) is success.
pub async fn run_uninstall_service(
    args: UninstallServiceArgs,
) -> Result<(), UserFacingError> {
    let plist_dir = match args.plist_dir {
        Some(p) => p,
        None => default_launch_agents_dir()?,
    };
    let plist_path = plist_dir.join(format!("{LAUNCHD_LABEL}.plist"));

    if !args.no_unload && plist_path.exists() {
        // Best-effort — a daemon that's already crashed yields
        // "boot-out failed" or similar; we don't want to fail the
        // uninstall because of that.
        let _ = launchctl_unload(&plist_path);
    }

    let removed = if plist_path.exists() {
        std::fs::remove_file(&plist_path).map_err(|source| {
            UserFacingError::ServiceCommandFailed {
                cmd: format!("rm {}", plist_path.display()),
                stderr: source.to_string(),
            }
        })?;
        true
    } else {
        false
    };

    output::emit_service_uninstalled(&plist_path, removed, args.json);
    Ok(())
}

/// Execute `chum daemon service-status`.
pub async fn run_service_status(args: ServiceStatusArgs) -> Result<(), UserFacingError> {
    let launchctl = args
        .launchctl_path
        .unwrap_or_else(|| PathBuf::from("launchctl"));
    let status = launchctl_list(&launchctl, LAUNCHD_LABEL);
    output::emit_service_status(&status, args.json);
    Ok(())
}

// ---------------------------------------------------------------
// Internals
// ---------------------------------------------------------------

fn default_chumd_path() -> Result<PathBuf, UserFacingError> {
    let chum_exe = std::env::current_exe().map_err(|source| UserFacingError::RootIo {
        path: PathBuf::from("<current_exe>"),
        source,
    })?;
    let parent = chum_exe.parent().ok_or_else(|| UserFacingError::RootIo {
        path: chum_exe.clone(),
        source: std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "current_exe has no parent",
        ),
    })?;
    let chumd = parent.join("chumd");
    if chumd.is_file() {
        chumd
            .canonicalize()
            .map_err(|source| UserFacingError::RootIo {
                path: chumd.clone(),
                source,
            })
    } else {
        Err(UserFacingError::ServiceCommandFailed {
            cmd: "resolve chumd path".to_string(),
            stderr: format!(
                "chumd not found at {} — pass --chumd-path to override",
                chumd.display()
            ),
        })
    }
}

fn default_launch_agents_dir() -> Result<PathBuf, UserFacingError> {
    let home = std::env::var("HOME").map_err(|_| UserFacingError::ChumHomeUnresolved)?;
    Ok(PathBuf::from(home).join("Library/LaunchAgents"))
}

fn default_log_dir() -> Result<PathBuf, UserFacingError> {
    let home = std::env::var("HOME").map_err(|_| UserFacingError::ChumHomeUnresolved)?;
    Ok(PathBuf::from(home).join("Library/Logs"))
}

fn set_mode_644(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644));
}

fn launchctl_load(plist_path: &Path) -> Result<(), String> {
    run_launchctl(&PathBuf::from("launchctl"), &["load", &plist_path.display().to_string()])
}

fn launchctl_unload(plist_path: &Path) -> Result<(), String> {
    run_launchctl(&PathBuf::from("launchctl"), &["unload", &plist_path.display().to_string()])
}

fn run_launchctl(bin: &Path, args: &[&str]) -> Result<(), String> {
    let output = StdCommand::new(bin)
        .args(args)
        .output()
        .map_err(|e| format!("spawn launchctl: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        return Err(format!(
            "launchctl {} exited {}: {}{}",
            args.join(" "),
            output.status.code().unwrap_or(-1),
            stderr,
            if stdout.is_empty() {
                String::new()
            } else {
                format!(" / stdout: {stdout}")
            },
        ));
    }
    Ok(())
}

/// Run `launchctl list <label>` and parse PID + LastExitStatus from
/// the OpenStep-format output. A non-zero exit (label not loaded)
/// yields `loaded = false`.
pub fn launchctl_list(launchctl: &Path, label: &str) -> ServiceStatus {
    let output = match StdCommand::new(launchctl).arg("list").arg(label).output() {
        Ok(o) => o,
        Err(_) => {
            return ServiceStatus {
                loaded: false,
                pid: None,
                last_exit_status: None,
            };
        }
    };
    if !output.status.success() {
        return ServiceStatus {
            loaded: false,
            pid: None,
            last_exit_status: None,
        };
    }
    let body = String::from_utf8_lossy(&output.stdout);
    ServiceStatus {
        loaded: true,
        pid: parse_int_field(&body, "PID"),
        last_exit_status: parse_int_field(&body, "LastExitStatus"),
    }
}

/// Find `"FIELD" = N;` in OpenStep plist text. Returns `None` if not
/// found or not an integer.
fn parse_int_field(body: &str, field: &str) -> Option<i32> {
    for line in body.lines() {
        let trimmed = line.trim();
        // Match shape:   "Field" = 12345;
        let key = format!("\"{field}\"");
        if let Some(rest) = trimmed.strip_prefix(&key) {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim();
                let n = rest.trim_end_matches(';').trim();
                if let Ok(v) = n.parse::<i32>() {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Minimal XML attribute / text escape — replaces the five characters
/// that could break parsing of a `<string>` value.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> ServiceConfig {
        ServiceConfig {
            chumd_path: PathBuf::from("/usr/local/bin/chumd"),
            chum_home: PathBuf::from("/Users/test/.chum"),
            plist_path: PathBuf::from("/Users/test/Library/LaunchAgents/cloud.chum.daemon.plist"),
            stdout_log: PathBuf::from("/Users/test/Library/Logs/chum-daemon.stdout.log"),
            stderr_log: PathBuf::from("/Users/test/Library/Logs/chum-daemon.stderr.log"),
            path_env: "/usr/local/bin:/usr/bin:/bin".to_string(),
        }
    }

    #[test]
    fn render_plist_contains_required_keys() {
        let body = sample_config().render_plist();
        // Label
        assert!(body.contains("<key>Label</key>"));
        assert!(body.contains(&format!("<string>{LAUNCHD_LABEL}</string>")));
        // ProgramArguments + --root
        assert!(body.contains("<key>ProgramArguments</key>"));
        assert!(body.contains("<string>/usr/local/bin/chumd</string>"));
        assert!(body.contains("<string>--root</string>"));
        assert!(body.contains("<string>/Users/test/.chum</string>"));
        // RunAtLoad + KeepAlive.SuccessfulExit=false
        assert!(body.contains("<key>RunAtLoad</key>"));
        assert!(body.contains("<true/>"));
        assert!(body.contains("<key>KeepAlive</key>"));
        assert!(body.contains("<key>SuccessfulExit</key>"));
        assert!(body.contains("<false/>"));
        // EnvironmentVariables baked in (no launchctl setenv)
        assert!(body.contains("<key>EnvironmentVariables</key>"));
        assert!(body.contains("<key>PATH</key>"));
        assert!(body.contains("<key>CHUM_HOME</key>"));
        // Log paths
        assert!(body.contains("<key>StandardOutPath</key>"));
        assert!(body.contains("chum-daemon.stdout.log"));
        assert!(body.contains("<key>StandardErrorPath</key>"));
        assert!(body.contains("chum-daemon.stderr.log"));
        // WorkingDirectory
        assert!(body.contains("<key>WorkingDirectory</key>"));
    }

    #[test]
    fn render_plist_starts_with_xml_declaration() {
        let body = sample_config().render_plist();
        assert!(body.starts_with("<?xml version=\"1.0\""));
        assert!(body.contains("PUBLIC \"-//Apple//DTD PLIST 1.0//EN\""));
    }

    #[test]
    fn xml_escape_replaces_dangerous_chars() {
        assert_eq!(xml_escape("a<b>c&d\"e'f"), "a&lt;b&gt;c&amp;d&quot;e&apos;f");
        assert_eq!(xml_escape("/usr/local/bin/chumd"), "/usr/local/bin/chumd");
    }

    #[test]
    fn parse_int_field_finds_pid_and_exit_status() {
        // Realistic OpenStep-format launchctl list output.
        let sample = r#"{
	"LimitLoadToSessionType" = "Aqua";
	"Label" = "cloud.chum.daemon";
	"OnDemand" = false;
	"LastExitStatus" = 0;
	"PID" = 12345;
	"Program" = "/usr/local/bin/chumd";
};"#;
        assert_eq!(parse_int_field(sample, "PID"), Some(12345));
        assert_eq!(parse_int_field(sample, "LastExitStatus"), Some(0));
        assert_eq!(parse_int_field(sample, "OnDemand"), None);  // not an int
        assert_eq!(parse_int_field(sample, "NotPresent"), None);
    }

    #[test]
    fn parse_int_field_handles_missing_pid() {
        // Daemon not currently running — launchctl omits PID.
        let sample = r#"{
	"Label" = "cloud.chum.daemon";
	"LastExitStatus" = 1;
};"#;
        assert_eq!(parse_int_field(sample, "PID"), None);
        assert_eq!(parse_int_field(sample, "LastExitStatus"), Some(1));
    }
}
