//! End-to-end test for `chum daemon install-service` /
//! `uninstall-service`.
//!
//! Drives the `chum` binary against a tempdir-rooted plist directory
//! using the `--plist-dir` + `--no-load` / `--no-unload` test
//! escape hatches, so no actual `launchctl` invocation happens and
//! `~/Library/LaunchAgents/` is never touched.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

const LABEL: &str = "cloud.chum.daemon";

fn chum_bin() -> &'static str {
    env!("CARGO_BIN_EXE_chum")
}

fn make_chum_root(tmp: &Path) -> PathBuf {
    let root = tmp.join("chum-home");
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn fake_chumd(tmp: &Path) -> PathBuf {
    let path = tmp.join("chumd-stub");
    std::fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.canonicalize().unwrap()
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(chum_bin())
        .args(args)
        .output()
        .expect("spawn chum")
}

#[test]
fn install_service_writes_plist_to_custom_dir() {
    let tmp = TempDir::new().unwrap();
    let chum_root = make_chum_root(tmp.path());
    let plist_dir = tmp.path().join("LaunchAgents");
    std::fs::create_dir_all(&plist_dir).unwrap();
    let log_dir = tmp.path().join("Logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    let chumd = fake_chumd(tmp.path());

    let out = run(&[
        "daemon",
        "install-service",
        "--chumd-path",
        &chumd.display().to_string(),
        "--root",
        &chum_root.display().to_string(),
        "--plist-dir",
        &plist_dir.display().to_string(),
        "--log-dir",
        &log_dir.display().to_string(),
        "--no-load",
    ]);
    assert!(
        out.status.success(),
        "install-service failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    let plist_path = plist_dir.join(format!("{LABEL}.plist"));
    assert!(plist_path.is_file(), "plist not written to {plist_path:?}");
    let body = std::fs::read_to_string(&plist_path).unwrap();
    assert!(body.contains(&format!("<string>{LABEL}</string>")));
    assert!(body.contains(&format!(
        "<string>{}</string>",
        chumd.display()
    )));
    assert!(body.contains(&format!(
        "<string>{}</string>",
        chum_root.display()
    )));
    assert!(body.contains("<key>RunAtLoad</key>"));
    assert!(body.contains("<true/>"));
    assert!(body.contains("<key>SuccessfulExit</key>"));
    assert!(body.contains("<false/>"));
    // Logs use the overridden log dir
    assert!(body.contains(&format!(
        "<string>{}/chum-daemon.stdout.log</string>",
        log_dir.display()
    )));
    assert!(body.contains(&format!(
        "<string>{}/chum-daemon.stderr.log</string>",
        log_dir.display()
    )));
}

#[test]
fn install_service_refuses_existing_without_force() {
    let tmp = TempDir::new().unwrap();
    let chum_root = make_chum_root(tmp.path());
    let plist_dir = tmp.path().join("LaunchAgents");
    std::fs::create_dir_all(&plist_dir).unwrap();
    let chumd = fake_chumd(tmp.path());

    let common = vec![
        "--chumd-path",
        chumd.to_str().unwrap(),
        "--root",
        chum_root.to_str().unwrap(),
        "--plist-dir",
        plist_dir.to_str().unwrap(),
        "--no-load",
    ];

    // First install — ok.
    let mut args = vec!["daemon", "install-service"];
    args.extend(common.iter().copied());
    let first = run(&args);
    assert!(first.status.success(), "first install must succeed");

    // Second install (no --force) — must fail.
    let second = run(&args);
    assert!(
        !second.status.success(),
        "second install without --force must fail"
    );
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("already exists") || stderr.contains("re-run with --force"),
        "expected explanatory error, got: {stderr}"
    );

    // Third install with --force — ok.
    let mut force_args = vec!["daemon", "install-service", "--force"];
    force_args.extend(common.iter().copied());
    let third = run(&force_args);
    assert!(
        third.status.success(),
        "install --force must succeed: stderr={}",
        String::from_utf8_lossy(&third.stderr),
    );
}

#[test]
fn uninstall_service_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let plist_dir = tmp.path().join("LaunchAgents");
    std::fs::create_dir_all(&plist_dir).unwrap();

    // Uninstall with no prior install — succeeds.
    let bare = run(&[
        "daemon",
        "uninstall-service",
        "--plist-dir",
        plist_dir.to_str().unwrap(),
        "--no-unload",
    ]);
    assert!(
        bare.status.success(),
        "uninstall with no prior plist must succeed (idempotent): stderr={}",
        String::from_utf8_lossy(&bare.stderr),
    );

    // Install then uninstall — file gone.
    let chum_root = make_chum_root(tmp.path());
    let chumd = fake_chumd(tmp.path());
    let install = run(&[
        "daemon",
        "install-service",
        "--chumd-path",
        chumd.to_str().unwrap(),
        "--root",
        chum_root.to_str().unwrap(),
        "--plist-dir",
        plist_dir.to_str().unwrap(),
        "--no-load",
    ]);
    assert!(install.status.success());
    let plist_path = plist_dir.join(format!("{LABEL}.plist"));
    assert!(plist_path.is_file());

    let uninstall = run(&[
        "daemon",
        "uninstall-service",
        "--plist-dir",
        plist_dir.to_str().unwrap(),
        "--no-unload",
    ]);
    assert!(
        uninstall.status.success(),
        "uninstall failed: stderr={}",
        String::from_utf8_lossy(&uninstall.stderr),
    );
    assert!(!plist_path.exists(), "plist must be removed after uninstall");
}

#[test]
fn install_service_json_envelope_shape() {
    let tmp = TempDir::new().unwrap();
    let chum_root = make_chum_root(tmp.path());
    let plist_dir = tmp.path().join("LaunchAgents");
    std::fs::create_dir_all(&plist_dir).unwrap();
    let chumd = fake_chumd(tmp.path());

    let out = run(&[
        "daemon",
        "install-service",
        "--chumd-path",
        chumd.to_str().unwrap(),
        "--root",
        chum_root.to_str().unwrap(),
        "--plist-dir",
        plist_dir.to_str().unwrap(),
        "--no-load",
        "--json",
    ]);
    assert!(out.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
    assert_eq!(parsed["status"], "ok");
    let installed = &parsed["service_installed"];
    assert_eq!(installed["label"], LABEL);
    assert_eq!(installed["loaded"], false);
    assert!(
        installed["plist_path"]
            .as_str()
            .is_some_and(|s| s.ends_with(&format!("{LABEL}.plist")))
    );
}
