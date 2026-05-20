//! Semantic validation for manifests beyond what serde catches.

use super::{Manifest, Source, Transport};
use crate::ManifestError;

/// Run semantic checks on a structurally-parsed manifest.
///
/// Callers are expected to have already run [`super::parse_str`] (or to
/// have constructed a [`Manifest`] in memory). The checks performed:
///
/// - `package.name` matches `[a-z][a-z0-9-]{0,62}`
/// - `package.version` is non-empty
/// - For [`Source::Binary`]: `url` starts with `http://` or `https://`,
///   and `checksum_sha256` is exactly 64 hex characters
/// - For [`Transport::Http`] / [`Transport::Sse`]: `bind` is not a
///   wildcard / unspecified address — CHUM is local-first, so binding
///   to `0.0.0.0`, `::`, `[::]`, or `*` is rejected
///
/// Validation **short-circuits** on the first failure. Tools that need
/// to surface every issue at once should call this iteratively or build
/// their own combined check pass.
///
/// # Errors
///
/// Returns the first failing check as the appropriate
/// [`ManifestError`] variant.
pub fn validate(manifest: &Manifest) -> Result<(), ManifestError> {
    validate_name(&manifest.package.name)?;
    validate_version(&manifest.package.version)?;
    validate_source(&manifest.source)?;
    validate_transport(&manifest.runtime.transport)?;
    Ok(())
}

fn validate_name(name: &str) -> Result<(), ManifestError> {
    if is_valid_name(name) {
        Ok(())
    } else {
        Err(ManifestError::InvalidName(name.to_string()))
    }
}

fn validate_version(version: &str) -> Result<(), ManifestError> {
    if version.is_empty() {
        Err(ManifestError::InvalidVersion(version.to_string()))
    } else {
        Ok(())
    }
}

fn validate_source(source: &Source) -> Result<(), ManifestError> {
    if let Source::Binary {
        url,
        checksum_sha256,
        ..
    } = source
    {
        if !looks_like_http_url(url) {
            return Err(ManifestError::InvalidUrl(url.clone()));
        }
        if !is_valid_sha256_hex(checksum_sha256) {
            return Err(ManifestError::InvalidChecksum(checksum_sha256.clone()));
        }
    }
    Ok(())
}

fn validate_transport(transport: &Transport) -> Result<(), ManifestError> {
    let bind = match transport {
        Transport::Stdio => return Ok(()),
        Transport::Http { bind, .. } | Transport::Sse { bind, .. } => bind,
    };
    if is_unsafe_bind_address(bind) {
        return Err(ManifestError::InvalidBindAddress(bind.clone()));
    }
    Ok(())
}

fn is_valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    if !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_valid_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn looks_like_http_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

fn is_unsafe_bind_address(bind: &str) -> bool {
    matches!(bind, "0.0.0.0" | "::" | "[::]" | "*")
}
