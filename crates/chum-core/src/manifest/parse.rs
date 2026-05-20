//! TOML parsing for CHUM manifests.
//!
//! Two-stage strategy: peek the top-level `schema_version` first so we can
//! reject unknown versions with a precise error before serde tries to
//! interpret fields the older parser doesn't recognise. The full parse
//! re-runs against the original input so toml's error spans point at the
//! source.

use serde::Deserialize;

use super::{Manifest, SCHEMA_VERSION};
use crate::ManifestError;

#[derive(Deserialize)]
struct SchemaVersionPeek {
    #[serde(default)]
    schema_version: String,
}

/// Parse a manifest from a TOML string.
///
/// Two-stage:
///
/// 1. Peek the top-level `schema_version` field. Missing →
///    [`ManifestError::MissingSchemaVersion`]; unrecognised →
///    [`ManifestError::UnsupportedSchemaVersion`].
/// 2. Full structural deserialise into [`Manifest`] (with
///    `deny_unknown_fields` on every known struct). Errors surface as
///    [`ManifestError::Toml`].
///
/// **No semantic validation** is performed here beyond what serde catches.
/// Call [`super::validate`] (or [`parse_and_validate`]) for name format,
/// URL scheme, checksum length, and bind-address checks.
///
/// # Errors
///
/// - [`ManifestError::Toml`] — TOML syntax error, or a required field is
///   missing on a known shape (e.g. a `Binary` source without
///   `checksum_sha256`). See `MANIFEST_SPEC.md` for the rationale on why
///   missing-required-field surfaces here rather than as a dedicated
///   variant in v0.1.
/// - [`ManifestError::MissingSchemaVersion`] — manifest does not declare a
///   top-level `schema_version`.
/// - [`ManifestError::UnsupportedSchemaVersion`] — manifest declares a
///   `schema_version` newer (or otherwise unrecognised) than this build.
pub fn parse_str(input: &str) -> Result<Manifest, ManifestError> {
    let peek: SchemaVersionPeek = toml::from_str(input)?;

    if peek.schema_version.is_empty() {
        return Err(ManifestError::MissingSchemaVersion);
    }
    if peek.schema_version != SCHEMA_VERSION {
        return Err(ManifestError::UnsupportedSchemaVersion(
            peek.schema_version,
        ));
    }

    let manifest: Manifest = toml::from_str(input)?;
    Ok(manifest)
}

/// Parse a manifest from a TOML string **and** run semantic validation.
///
/// Convenience equivalent to [`parse_str`] followed by [`super::validate`].
/// Use this when you want a single call to surface either a parse error
/// or a semantic violation.
///
/// # Errors
///
/// Any [`ManifestError`] variant that [`parse_str`] or
/// [`super::validate`] can produce.
pub fn parse_and_validate(input: &str) -> Result<Manifest, ManifestError> {
    let manifest = parse_str(input)?;
    super::validate(&manifest)?;
    Ok(manifest)
}
