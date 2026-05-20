//! Binary source handler: fetch, verify SHA-256, extract or copy.

use std::path::Path;

use chum_core::Manifest;
use sha2::{Digest, Sha256};

use crate::fetcher::Fetcher;
use crate::install::{InstalledArtifact, SourceKind};
use crate::InstallError;

/// Install a [`chum_core::manifest::Source::Binary`] manifest.
///
/// # Steps
///
/// 1. Fetch `url` via the supplied [`Fetcher`].
/// 2. Compute SHA-256 of the response body.
/// 3. Compare the lowercase-hex digest against
///    `expected_checksum.to_lowercase()`. **Mismatch is a hard reject** —
///    no fallback, no retry. The bytes are dropped without ever
///    touching the filesystem.
/// 4. Detect archive format by URL extension and extract; otherwise
///    place the raw bytes verbatim at `install_dir/bin/<filename>`.
///
/// # Archive detection (v0.1, extension-only)
///
/// - `.tar.gz` / `.tgz` → tar + gzip extraction
/// - `.tar` → tar extraction
/// - `.zip` → zip extraction
/// - any other suffix → treated as a single binary
///
/// Query strings are stripped before extension matching. v0.1 does
/// **not** inspect content-type or magic bytes; the manifest is
/// expected to use a known extension. This is documented in
/// `docs/MANIFEST_SPEC.md`.
///
// TODO(chum-v0.2): execute manifest-declared post-install scripts.
// v0.1 explicitly does not run anything from the downloaded archive —
// post-install scripts are an attack-surface decision that needs a
// permission-model conversation, which is v0.2 territory.
//
// TODO(chum-v0.2): streaming SHA-256 verification so the body is hashed
// while bytes are received instead of after buffering. Pair with the
// MAX_BODY_BYTES streaming cap in fetcher.rs.
//
/// # Errors
///
/// - [`InstallError::FetchFailed`] from the fetcher.
/// - [`InstallError::ChecksumMismatch`] on SHA-256 mismatch — the only
///   security-critical reject in this function.
/// - [`InstallError::ExtractFailed`] on archive corruption or an entry
///   that escapes the extraction root.
/// - [`InstallError::Io`] for filesystem operations.
pub async fn install_binary<F: Fetcher>(
    manifest: &Manifest,
    install_dir: &Path,
    url: &str,
    expected_checksum: &str,
    fetcher: &F,
) -> Result<InstalledArtifact, InstallError> {
    let bytes = fetcher.fetch(url).await?;

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual = hex_lower(&hasher.finalize());
    let expected = expected_checksum.to_lowercase();

    if actual != expected {
        return Err(InstallError::ChecksumMismatch { expected, actual });
    }

    let bin_dir = install_dir.join("bin");
    tokio::fs::create_dir_all(&bin_dir).await?;

    let path_segment = url.split('?').next().unwrap_or(url);

    if path_segment.ends_with(".tar.gz") || path_segment.ends_with(".tgz") {
        extract_tar_gz(&bytes, &bin_dir)?;
    } else if path_segment.ends_with(".tar") {
        extract_tar(&bytes, &bin_dir)?;
    } else if path_segment.ends_with(".zip") {
        extract_zip(&bytes, &bin_dir)?;
    } else {
        let filename = filename_from_url(path_segment);
        tokio::fs::write(bin_dir.join(filename), &bytes).await?;
    }

    Ok(InstalledArtifact {
        name: manifest.package.name.clone(),
        version: manifest.package.version.clone(),
        install_dir: install_dir.to_path_buf(),
        entrypoint: bin_dir,
        source_kind: SourceKind::Binary,
    })
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn filename_from_url(path_segment: &str) -> String {
    path_segment
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("artifact")
        .to_string()
}

fn extract_tar_gz(bytes: &[u8], dest: &Path) -> Result<(), InstallError> {
    let gz = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(gz);
    archive
        .unpack(dest)
        .map_err(|e| InstallError::ExtractFailed(e.to_string()))
}

fn extract_tar(bytes: &[u8], dest: &Path) -> Result<(), InstallError> {
    let mut archive = tar::Archive::new(bytes);
    archive
        .unpack(dest)
        .map_err(|e| InstallError::ExtractFailed(e.to_string()))
}

fn extract_zip(bytes: &[u8], dest: &Path) -> Result<(), InstallError> {
    let reader = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| InstallError::ExtractFailed(e.to_string()))?;
    archive
        .extract(dest)
        .map_err(|e| InstallError::ExtractFailed(e.to_string()))
}
