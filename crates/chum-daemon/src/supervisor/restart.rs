//! Restart policy + exponential backoff.
//!
//! The backoff schedule is a pure function of the attempt number, so
//! no per-process queue or persistent state is needed — see
//! [`BackoffPolicy::delay_for`].

use std::time::Duration;

use chum_core::manifest::RestartPolicy;

/// Backoff schedule for restart attempts.
///
/// The v0.1 standard schedule doubles from `base` up to `cap`:
/// `base, 2·base, 4·base, ... , cap, cap, cap, ...`. Tests construct
/// faster variants to keep restart-count assertions quick.
///
/// Bounds: both `base` and `cap` are at least `1ns` semantically;
/// `cap` is the ceiling for any single delay, including the first.
#[derive(Debug, Clone, Copy)]
pub struct BackoffPolicy {
    /// First-attempt delay. The n-th attempt waits `min(cap, base * 2^(n-1))`.
    pub base: Duration,
    /// Hard ceiling — no delay ever exceeds this.
    pub cap: Duration,
}

impl BackoffPolicy {
    /// v0.1 production schedule: 1s, 2s, 4s, 8s, 16s, 16s, …
    pub fn standard() -> Self {
        Self {
            base: Duration::from_secs(1),
            cap: Duration::from_secs(16),
        }
    }

    /// Compute the wait before restart attempt `attempt`.
    ///
    /// `attempt` is 1-indexed — the first restart uses `attempt = 1`
    /// and waits `base`. Saturates at `cap` for large attempts and
    /// never panics on overflow.
    pub fn delay_for(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return self.base.min(self.cap);
        }
        // Shift one bit per attempt past the first; clamp to 30 to
        // keep within u32 and well past the saturation point.
        let shift = attempt.saturating_sub(1).min(30);
        let factor: u32 = 1u32.checked_shl(shift).unwrap_or(u32::MAX);
        let raw = self.base.saturating_mul(factor);
        raw.min(self.cap)
    }
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        Self::standard()
    }
}

/// Decide whether a child that just exited should be restarted under
/// the manifest's restart policy.
///
/// - `Always` → restart on any exit, including clean (`code == 0`).
/// - `OnFailure` → restart only on a non-zero exit or a signal
///   termination (no exit code).
/// - `Never` → never restart.
pub(crate) fn should_restart(policy: RestartPolicy, exit_code: Option<i32>) -> bool {
    match policy {
        RestartPolicy::Always => true,
        RestartPolicy::OnFailure => match exit_code {
            Some(0) => false,
            // Non-zero exit OR signal termination (None) → failure.
            _ => true,
        },
        RestartPolicy::Never => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_schedule_doubles_then_caps() {
        let p = BackoffPolicy::standard();
        assert_eq!(p.delay_for(1), Duration::from_secs(1));
        assert_eq!(p.delay_for(2), Duration::from_secs(2));
        assert_eq!(p.delay_for(3), Duration::from_secs(4));
        assert_eq!(p.delay_for(4), Duration::from_secs(8));
        assert_eq!(p.delay_for(5), Duration::from_secs(16));
        // Cap kicks in.
        assert_eq!(p.delay_for(6), Duration::from_secs(16));
        assert_eq!(p.delay_for(100), Duration::from_secs(16));
    }

    #[test]
    fn delay_for_saturates_safely_on_huge_attempt() {
        let p = BackoffPolicy::standard();
        // Must not panic.
        let _ = p.delay_for(u32::MAX);
    }

    #[test]
    fn fast_schedule_for_tests() {
        let p = BackoffPolicy {
            base: Duration::from_millis(50),
            cap: Duration::from_millis(200),
        };
        assert_eq!(p.delay_for(1), Duration::from_millis(50));
        assert_eq!(p.delay_for(2), Duration::from_millis(100));
        assert_eq!(p.delay_for(3), Duration::from_millis(200));
        assert_eq!(p.delay_for(10), Duration::from_millis(200));
    }

    #[test]
    fn always_restarts_on_any_exit() {
        assert!(should_restart(RestartPolicy::Always, Some(0)));
        assert!(should_restart(RestartPolicy::Always, Some(1)));
        assert!(should_restart(RestartPolicy::Always, None));
    }

    #[test]
    fn on_failure_skips_clean_exit() {
        assert!(!should_restart(RestartPolicy::OnFailure, Some(0)));
        assert!(should_restart(RestartPolicy::OnFailure, Some(1)));
        assert!(should_restart(RestartPolicy::OnFailure, Some(137)));
        assert!(should_restart(RestartPolicy::OnFailure, None));
    }

    #[test]
    fn never_means_never() {
        assert!(!should_restart(RestartPolicy::Never, Some(0)));
        assert!(!should_restart(RestartPolicy::Never, Some(1)));
        assert!(!should_restart(RestartPolicy::Never, None));
    }
}
