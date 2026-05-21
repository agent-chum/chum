//! Typed schema for the `[permissions]` block of a manifest.
//!
//! v0.1 is **bookkeeping only**: the manifest declares what
//! categories of capability the MCP server expects to use, the user
//! grants them via `chum permit`, and the daemon refuses to spawn
//! if anything declared is ungranted. Spawned processes are NOT
//! actually sandboxed in v0.1 — see `docs/BROKER_DESIGN.md` for the
//! v0.2 enforcement model.
//!
//! Every category and every leaf field defaults to empty. A manifest
//! with no `[permissions]` block parses into `Permissions::default()`,
//! the broker sees zero required permissions, and the spawn passes
//! through. Pre-broker installs keep working without modification.

use serde::{Deserialize, Serialize};

/// The five string-form permission kinds. These are the wire codes
/// stored in the registry's `permission_grants.kind` column and the
/// values accepted by `chum permit --grant <kind>=<value>`.
///
/// Exact-string match is the v0.1 semantic; wildcards / prefix
/// matching land in v0.2 with real enforcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionKind {
    /// Filesystem read access to a specific absolute path.
    FilesystemRead,
    /// Filesystem write access to a specific absolute path.
    FilesystemWrite,
    /// Outbound network access to a specific host (no scheme, no port).
    NetworkOutbound,
    /// Read access to a specific environment-variable name.
    EnvRead,
    /// Subprocess execution by program name or absolute path.
    SubprocessExec,
}

impl PermissionKind {
    /// Stable string form used on the wire, in the registry, and on
    /// the cli. The five values: `filesystem.read`, `filesystem.write`,
    /// `network.outbound`, `env.read`, `subprocess.exec`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FilesystemRead => "filesystem.read",
            Self::FilesystemWrite => "filesystem.write",
            Self::NetworkOutbound => "network.outbound",
            Self::EnvRead => "env.read",
            Self::SubprocessExec => "subprocess.exec",
        }
    }

    /// Parse a wire-form kind string into the typed enum. Returns
    /// `None` for any string not in the five known values.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "filesystem.read" => Some(Self::FilesystemRead),
            "filesystem.write" => Some(Self::FilesystemWrite),
            "network.outbound" => Some(Self::NetworkOutbound),
            "env.read" => Some(Self::EnvRead),
            "subprocess.exec" => Some(Self::SubprocessExec),
            _ => None,
        }
    }

    /// All five kinds, in stable order. Useful for help text and
    /// `chum permissions` rendering.
    pub fn all() -> [Self; 5] {
        [
            Self::FilesystemRead,
            Self::FilesystemWrite,
            Self::NetworkOutbound,
            Self::EnvRead,
            Self::SubprocessExec,
        ]
    }
}

impl std::fmt::Display for PermissionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A flat (kind, value) pair representing one capability the manifest
/// declares or one grant the user has issued.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PermissionRequirement {
    /// Permission category + operation.
    pub kind: PermissionKind,
    /// Free-form value — absolute path, host, env var name, or
    /// program name depending on `kind`. Exact-string matched
    /// against grants.
    pub value: String,
}

/// Top-level `[permissions]` block in a manifest.
///
/// Every subtable is `#[serde(default)]` so partial declarations
/// (only `filesystem`, only `env`) inherit empty for the rest.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permissions {
    /// Filesystem read / write declarations.
    #[serde(default)]
    pub filesystem: FilesystemPermissions,
    /// Network outbound declarations.
    #[serde(default)]
    pub network: NetworkPermissions,
    /// Environment-variable read declarations.
    #[serde(default)]
    pub env: EnvPermissions,
    /// Subprocess execution declarations.
    #[serde(default)]
    pub subprocess: SubprocessPermissions,
}

/// Filesystem read / write declarations.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilesystemPermissions {
    /// Paths the MCP server expects to read.
    #[serde(default)]
    pub read: Vec<String>,
    /// Paths the MCP server expects to write.
    #[serde(default)]
    pub write: Vec<String>,
}

/// Network outbound declarations.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkPermissions {
    /// Hosts the MCP server expects to connect to.
    #[serde(default)]
    pub outbound: Vec<String>,
}

/// Environment-variable read declarations.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvPermissions {
    /// Env var names the MCP server expects to read.
    #[serde(default)]
    pub read: Vec<String>,
}

/// Subprocess execution declarations.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubprocessPermissions {
    /// Program names or absolute paths the MCP server expects to
    /// invoke as child processes.
    #[serde(default)]
    pub exec: Vec<String>,
}

impl Permissions {
    /// Iterate the flat (kind, value) requirement list this manifest
    /// declares. The broker matches each requirement against the
    /// user's granted set.
    ///
    /// Order is stable: filesystem.read entries first (in declared
    /// order), then filesystem.write, network.outbound, env.read,
    /// subprocess.exec. Useful for `chum permissions` rendering.
    pub fn iter_requirements(&self) -> impl Iterator<Item = PermissionRequirement> + '_ {
        let fs_read = self
            .filesystem
            .read
            .iter()
            .map(|v| PermissionRequirement {
                kind: PermissionKind::FilesystemRead,
                value: v.clone(),
            });
        let fs_write = self
            .filesystem
            .write
            .iter()
            .map(|v| PermissionRequirement {
                kind: PermissionKind::FilesystemWrite,
                value: v.clone(),
            });
        let net = self
            .network
            .outbound
            .iter()
            .map(|v| PermissionRequirement {
                kind: PermissionKind::NetworkOutbound,
                value: v.clone(),
            });
        let env = self.env.read.iter().map(|v| PermissionRequirement {
            kind: PermissionKind::EnvRead,
            value: v.clone(),
        });
        let subproc = self
            .subprocess
            .exec
            .iter()
            .map(|v| PermissionRequirement {
                kind: PermissionKind::SubprocessExec,
                value: v.clone(),
            });
        fs_read.chain(fs_write).chain(net).chain(env).chain(subproc)
    }

    /// `true` when no permissions are declared in any category.
    /// Manifests without a `[permissions]` block hit this path.
    pub fn is_empty(&self) -> bool {
        self.filesystem.read.is_empty()
            && self.filesystem.write.is_empty()
            && self.network.outbound.is_empty()
            && self.env.read.is_empty()
            && self.subprocess.exec.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_kind_round_trip_strings() {
        for k in PermissionKind::all() {
            assert_eq!(PermissionKind::from_str(k.as_str()), Some(k));
        }
        assert_eq!(PermissionKind::from_str("nonsense"), None);
        assert_eq!(PermissionKind::from_str("filesystem"), None); // missing op
    }

    #[test]
    fn default_permissions_is_empty() {
        let p = Permissions::default();
        assert!(p.is_empty());
        assert_eq!(p.iter_requirements().count(), 0);
    }

    #[test]
    fn iter_requirements_preserves_declared_order() {
        let p = Permissions {
            filesystem: FilesystemPermissions {
                read: vec!["/a".into(), "/b".into()],
                write: vec!["/w".into()],
            },
            network: NetworkPermissions {
                outbound: vec!["api.example.com".into()],
            },
            env: EnvPermissions {
                read: vec!["FOO".into()],
            },
            subprocess: SubprocessPermissions {
                exec: vec!["git".into()],
            },
        };
        let reqs: Vec<_> = p.iter_requirements().collect();
        assert_eq!(reqs.len(), 6);
        assert_eq!(reqs[0].kind, PermissionKind::FilesystemRead);
        assert_eq!(reqs[0].value, "/a");
        assert_eq!(reqs[1].kind, PermissionKind::FilesystemRead);
        assert_eq!(reqs[1].value, "/b");
        assert_eq!(reqs[2].kind, PermissionKind::FilesystemWrite);
        assert_eq!(reqs[3].kind, PermissionKind::NetworkOutbound);
        assert_eq!(reqs[4].kind, PermissionKind::EnvRead);
        assert_eq!(reqs[5].kind, PermissionKind::SubprocessExec);
    }

    #[test]
    fn permissions_parses_from_toml() {
        let raw = r#"
[filesystem]
read = ["/Users/x/Documents"]
write = ["/tmp/chum-workspace"]

[network]
outbound = ["api.search.brave.com"]

[env]
read = ["BRAVE_API_KEY"]

[subprocess]
exec = ["git"]
"#;
        let p: Permissions = toml::from_str(raw).unwrap();
        assert_eq!(p.filesystem.read, vec!["/Users/x/Documents"]);
        assert_eq!(p.filesystem.write, vec!["/tmp/chum-workspace"]);
        assert_eq!(p.network.outbound, vec!["api.search.brave.com"]);
        assert_eq!(p.env.read, vec!["BRAVE_API_KEY"]);
        assert_eq!(p.subprocess.exec, vec!["git"]);
        assert_eq!(p.iter_requirements().count(), 5);
    }

    #[test]
    fn permissions_partial_subtables_default_others() {
        let raw = r#"
[network]
outbound = ["api.example.com"]
"#;
        let p: Permissions = toml::from_str(raw).unwrap();
        assert!(p.filesystem.read.is_empty());
        assert!(p.filesystem.write.is_empty());
        assert_eq!(p.network.outbound, vec!["api.example.com"]);
        assert!(p.env.read.is_empty());
        assert!(p.subprocess.exec.is_empty());
    }

    #[test]
    fn permissions_rejects_unknown_fields() {
        let raw = r#"
[filesystem]
exec = ["/oops"]
"#;
        let err = toml::from_str::<Permissions>(raw)
            .expect_err("unknown field on filesystem must fail");
        let msg = err.to_string();
        assert!(msg.contains("exec"), "expected mention of 'exec': {msg}");
    }
}
