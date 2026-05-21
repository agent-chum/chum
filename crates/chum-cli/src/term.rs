//! ANSI color helpers for tty output. No external crate — bare
//! escape codes inline. Auto-disables when stdout is not a tty or
//! when `NO_COLOR` is set in the environment
//! (https://no-color.org/), with a `--no-color` flag for explicit
//! opt-out on individual commands.

use std::io::IsTerminal;

const RESET: &str = "\x1b[0m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const GRAY: &str = "\x1b[90m";
const BLUE: &str = "\x1b[34m";
const MAGENTA: &str = "\x1b[35m";

/// Decide whether to emit ANSI escape codes.
///
/// Precedence: `--no-color` flag wins, then `NO_COLOR` env var, then
/// stdout-is-a-tty.
pub fn color_enabled(no_color_flag: bool) -> bool {
    if no_color_flag {
        return false;
    }
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    std::io::stdout().is_terminal()
}

/// Colorize a `ProcessStatus`-style string. Falls back to the input
/// unchanged when `enabled = false` or the status isn't one of the
/// known v0.1 lifecycle strings.
pub fn colorize_status(s: &str, enabled: bool) -> String {
    if !enabled {
        return s.to_string();
    }
    let color = match s {
        "running" => GREEN,
        "failed" => RED,
        "stopped" => GRAY,
        "restarting" => YELLOW,
        "starting" => CYAN,
        _ => return s.to_string(),
    };
    format!("{color}{s}{RESET}")
}

/// Colorize a `SourceKind`-style string for the `chum list` KIND
/// column. Falls back to unchanged when `enabled = false`.
pub fn colorize_kind(s: &str, enabled: bool) -> String {
    if !enabled {
        return s.to_string();
    }
    let color = match s {
        "npm" => BLUE,
        "local" => GREEN,
        "binary" => MAGENTA,
        _ => return s.to_string(),
    };
    format!("{color}{s}{RESET}")
}
