//! `chum-broker` — capability validation for the v0.1 daemon spawn
//! pipeline.
//!
//! Per `docs/BROKER_DESIGN.md`, v0.1 is **bookkeeping only**:
//! [`validate`] takes a manifest's declared permissions plus the
//! user-issued grants from the registry and returns a verdict the
//! daemon's spawn handler refuses on. Spawned processes are NOT
//! sandboxed in v0.1 — real enforcement (sandbox-exec, env
//! scrubbing, network filtering) lands in v0.2.
//!
//! Exact-string match in v0.1. A grant for `/Users/x` does NOT cover
//! `/Users/x/Documents` — they have to be granted separately.
//! Wildcards and prefix-matching land in v0.2 with the enforcement
//! model that justifies fuzziness.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashSet;

use chum_core::{PermissionRequirement, Permissions};
use chum_registry::Grant;

/// Verdict returned by [`validate`].
///
/// `Deny` carries the unmet requirements so the cli can render a
/// `chum permit --grant <kind>=<value>` hint per missing item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerVerdict {
    /// Every declared permission is covered by an existing grant
    /// (or the manifest declares no permissions, which is the
    /// pre-broker default).
    Allow,
    /// At least one declared permission has no matching grant.
    /// Spawn must be refused.
    Deny {
        /// Declared-but-not-granted requirements, in
        /// `Permissions::iter_requirements` order.
        unmet: Vec<PermissionRequirement>,
    },
}

/// Validate that every permission declared by `required` has a
/// matching grant in `granted`.
///
/// Matching semantics: **exact string** on both `kind` and `value`.
/// v0.2 introduces wildcard / prefix matching alongside real
/// enforcement; v0.1 prioritises auditability — every grant is
/// recorded, every check is unambiguous.
///
/// Empty `required` (the `Permissions::default()` case for manifests
/// without a `[permissions]` block) is always `Allow`. The broker is
/// transparent to pre-broker installs.
///
/// **Extra grants beyond what the manifest declares are silently
/// allowed.** The user has the prerogative to grant more than the
/// manifest asks for — the broker validates "are required permissions
/// covered?", not "are grants minimal?" v0.2 may add a
/// `--strict-grants` flag if a real audit need surfaces.
// TODO(chum-v0.2): wildcard / prefix matching for filesystem paths
// and network hosts (e.g., `*.anthropic.com`, `/Users/x` covering
// `/Users/x/Documents`). Today exact-string only.
pub fn validate(required: &Permissions, granted: &[Grant]) -> BrokerVerdict {
    if required.is_empty() {
        return BrokerVerdict::Allow;
    }
    let granted_set: HashSet<(&str, &str)> = granted
        .iter()
        .map(|g| (g.kind.as_str(), g.value.as_str()))
        .collect();
    let unmet: Vec<PermissionRequirement> = required
        .iter_requirements()
        .filter(|req| !granted_set.contains(&(req.kind.as_str(), req.value.as_str())))
        .collect();
    if unmet.is_empty() {
        BrokerVerdict::Allow
    } else {
        BrokerVerdict::Deny { unmet }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chum_core::{
        EnvPermissions, FilesystemPermissions, NetworkPermissions, PermissionKind,
    };
    use chrono::Utc;

    fn g(kind: &str, value: &str) -> Grant {
        Grant {
            kind: kind.to_string(),
            value: value.to_string(),
            granted_at: Utc::now(),
        }
    }

    #[test]
    fn empty_required_always_allows() {
        assert_eq!(validate(&Permissions::default(), &[]), BrokerVerdict::Allow);
        // Extra grants on a no-requirements manifest are silently allowed.
        assert_eq!(
            validate(
                &Permissions::default(),
                &[g("env.read", "ANYTHING")],
            ),
            BrokerVerdict::Allow,
        );
    }

    #[test]
    fn allow_when_every_required_is_granted() {
        let required = Permissions {
            filesystem: FilesystemPermissions {
                read: vec!["/x".into()],
                write: vec![],
            },
            env: EnvPermissions {
                read: vec!["FOO".into()],
            },
            ..Default::default()
        };
        let granted = vec![g("filesystem.read", "/x"), g("env.read", "FOO")];
        assert_eq!(validate(&required, &granted), BrokerVerdict::Allow);
    }

    #[test]
    fn deny_lists_every_unmet_requirement() {
        let required = Permissions {
            filesystem: FilesystemPermissions {
                read: vec!["/x".into(), "/y".into()],
                write: vec!["/z".into()],
            },
            env: EnvPermissions {
                read: vec!["FOO".into()],
            },
            ..Default::default()
        };
        // Grant only one of the four — the other three should appear
        // in the Deny.unmet list.
        let granted = vec![g("filesystem.read", "/x")];
        match validate(&required, &granted) {
            BrokerVerdict::Deny { unmet } => {
                assert_eq!(unmet.len(), 3);
                let kinds: Vec<_> = unmet.iter().map(|r| (r.kind, r.value.as_str())).collect();
                assert!(kinds.contains(&(PermissionKind::FilesystemRead, "/y")));
                assert!(kinds.contains(&(PermissionKind::FilesystemWrite, "/z")));
                assert!(kinds.contains(&(PermissionKind::EnvRead, "FOO")));
            }
            BrokerVerdict::Allow => panic!("expected Deny, got Allow"),
        }
    }

    #[test]
    fn exact_string_match_only() {
        // Grant `/Users/x` should NOT cover `/Users/x/Documents` in v0.1.
        let required = Permissions {
            filesystem: FilesystemPermissions {
                read: vec!["/Users/x/Documents".into()],
                write: vec![],
            },
            ..Default::default()
        };
        let granted = vec![g("filesystem.read", "/Users/x")];
        match validate(&required, &granted) {
            BrokerVerdict::Deny { unmet } => {
                assert_eq!(unmet.len(), 1);
                assert_eq!(unmet[0].kind, PermissionKind::FilesystemRead);
                assert_eq!(unmet[0].value, "/Users/x/Documents");
            }
            BrokerVerdict::Allow => panic!("v0.1 prefix matching is intentionally not implemented"),
        }
    }

    #[test]
    fn wrong_kind_does_not_cover() {
        // Granting filesystem.read=/x must not cover filesystem.write=/x.
        let required = Permissions {
            filesystem: FilesystemPermissions {
                read: vec![],
                write: vec!["/x".into()],
            },
            ..Default::default()
        };
        let granted = vec![g("filesystem.read", "/x")];
        match validate(&required, &granted) {
            BrokerVerdict::Deny { unmet } => {
                assert_eq!(unmet.len(), 1);
                assert_eq!(unmet[0].kind, PermissionKind::FilesystemWrite);
                assert_eq!(unmet[0].value, "/x");
            }
            BrokerVerdict::Allow => panic!("kinds must not cross-cover"),
        }
    }

    #[test]
    fn extra_grants_silently_allowed() {
        // Grant covers required, plus an extra grant that isn't declared.
        let required = Permissions {
            env: EnvPermissions {
                read: vec!["FOO".into()],
            },
            ..Default::default()
        };
        let granted = vec![
            g("env.read", "FOO"),
            g("env.read", "EXTRA_NOT_DECLARED"),
            g("network.outbound", "evil.example.com"),
        ];
        assert_eq!(validate(&required, &granted), BrokerVerdict::Allow);
    }

    #[test]
    fn network_outbound_exact_string() {
        let required = Permissions {
            network: NetworkPermissions {
                outbound: vec!["api.anthropic.com".into()],
            },
            ..Default::default()
        };
        // Grant for the parent domain does NOT cover.
        let granted = vec![g("network.outbound", "anthropic.com")];
        match validate(&required, &granted) {
            BrokerVerdict::Deny { unmet } => {
                assert_eq!(unmet.len(), 1);
                assert_eq!(unmet[0].value, "api.anthropic.com");
            }
            BrokerVerdict::Allow => panic!("v0.1 has no domain hierarchy"),
        }
    }
}
