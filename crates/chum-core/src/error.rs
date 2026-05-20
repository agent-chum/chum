//! Errors raised by manifest parsing and validation.

use thiserror::Error;

/// Errors returned by [`crate::manifest::parse_str`],
/// [`crate::manifest::validate`], and related entry points.
///
/// Variants distinguish syntactic failures from the underlying TOML parser
/// from semantic failures surfaced after a successful structural parse.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// The TOML failed to parse, either because the input was syntactically
    /// invalid or because a required field for the declared shape was
    /// missing. Missing required fields on a known source kind (e.g. a
    /// binary source without `checksum_sha256`) surface here in v0.1 — see
    /// MANIFEST_SPEC.md for the rationale.
    #[error("failed to parse manifest TOML: {0}")]
    Toml(#[from] toml::de::Error),

    /// The manifest could not be serialised back to TOML.
    #[error("failed to serialise manifest to TOML: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    /// The manifest did not declare a top-level `schema_version`.
    #[error("manifest is missing required field `schema_version`")]
    MissingSchemaVersion,

    /// The manifest declared a `schema_version` this build of chum-core
    /// does not recognise. Bumped at every breaking schema change.
    #[error("unsupported schema_version `{0}`; this build of chum-core understands `0.1`")]
    UnsupportedSchemaVersion(String),

    /// `package.name` failed the name format check.
    #[error("invalid package name `{0}`: must match `[a-z][a-z0-9-]{{0,62}}`")]
    InvalidName(String),

    /// `package.version` was empty.
    #[error("invalid package version `{0}`: must be non-empty")]
    InvalidVersion(String),

    /// A binary source `checksum_sha256` did not look like a 64-character
    /// lowercase hex SHA-256 digest.
    #[error("invalid sha256 checksum `{0}`: expected 64 hex characters")]
    InvalidChecksum(String),

    /// A binary source `url` did not start with `http://` or `https://`.
    #[error("invalid url `{0}`: must start with http:// or https://")]
    InvalidUrl(String),

    /// An HTTP or SSE transport declared a non-loopback bind address.
    /// Local-first means local-bound; `0.0.0.0` and `::` are rejected.
    #[error("invalid bind address `{0}`: CHUM is local-first; bind to a loopback address")]
    InvalidBindAddress(String),
}
