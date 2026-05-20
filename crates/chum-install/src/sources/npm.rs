//! Npm source handler: subprocess install via `npm install --prefix DIR pkg@ver`.

use std::path::Path;
use std::process::Stdio;

use chum_core::Manifest;
use tokio::process::Command;

use crate::install::{InstalledArtifact, SourceKind};
use crate::InstallError;

/// Install a [`chum_core::manifest::Source::Npm`] manifest by invoking
/// `npm install --prefix DIR <package>@<version>`.
///
/// # `npm_command`
///
/// The first element is the executable; subsequent elements are
/// pre-args inserted before `install`. The production default is
/// `["npm"]`. Integration tests override the slice — e.g.
/// `["/bin/sh", "<fake-npm-script>"]` — so the same code path can be
/// exercised without touching the host machine's PATH or installing a
/// real npm.
///
/// If `npm_command` is empty, falls back to `"npm"` as the executable.
///
/// # Behaviour
///
/// - stdin is closed (`Stdio::null`) — npm should not prompt
///   interactively from within an automated install.
/// - stdout and stderr are captured. On non-zero exit, stderr is
///   surfaced in [`InstallError::SubprocessFailed::stderr`].
/// - `kill_on_drop(true)` so a cancelled install future does not leak
///   a running npm process.
///
/// # Errors
///
/// - [`InstallError::MissingTool`] when `Command::spawn` returns
///   `ErrorKind::NotFound` — i.e., the executable isn't on PATH.
/// - [`InstallError::SubprocessFailed`] on non-zero exit.
/// - [`InstallError::Io`] for other spawn failures.
///
// TODO(chum-v0.2): execute manifest-declared post-install scripts. v0.1
// trusts whatever npm does internally (it runs the package's own
// install lifecycle hooks via `npm install`), but we do not add any
// extra steps on top.
pub async fn install_npm(
    manifest: &Manifest,
    install_dir: &Path,
    package: &str,
    version: &str,
    npm_command: &[String],
) -> Result<InstalledArtifact, InstallError> {
    let cmd_binary: &str = npm_command
        .first()
        .map(String::as_str)
        .unwrap_or("npm");
    let cmd_extra_args = npm_command.get(1..).unwrap_or(&[]);

    let pkg_spec = format!("{package}@{version}");

    let mut cmd = Command::new(cmd_binary);
    cmd.args(cmd_extra_args.iter())
        .arg("install")
        .arg("--prefix")
        .arg(install_dir)
        .arg(&pkg_spec)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let output = match cmd.output().await {
        Ok(out) => out,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(InstallError::MissingTool {
                tool: cmd_binary.to_string(),
            });
        }
        Err(e) => return Err(InstallError::Io(e)),
    };

    if !output.status.success() {
        let exit = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let cmd_display = render_command(cmd_binary, cmd_extra_args, install_dir, &pkg_spec);
        return Err(InstallError::SubprocessFailed {
            cmd: cmd_display,
            exit,
            stderr,
        });
    }

    // For scoped packages (`@scope/name`), npm preserves the slash in the
    // installed path — `node_modules/@scope/name/`. `Path::join` handles
    // that correctly.
    let entrypoint = install_dir.join("node_modules").join(package);

    Ok(InstalledArtifact {
        name: manifest.package.name.clone(),
        version: manifest.package.version.clone(),
        install_dir: install_dir.to_path_buf(),
        entrypoint,
        source_kind: SourceKind::Npm,
    })
}

fn render_command(
    binary: &str,
    extra: &[String],
    install_dir: &Path,
    pkg_spec: &str,
) -> String {
    let mut out = String::from(binary);
    for a in extra {
        out.push(' ');
        out.push_str(a);
    }
    out.push_str(" install --prefix ");
    out.push_str(&install_dir.to_string_lossy());
    out.push(' ');
    out.push_str(pkg_spec);
    out
}
