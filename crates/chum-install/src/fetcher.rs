//! HTTP fetcher abstraction for binary source installs.
//!
//! Production code uses [`ReqwestFetcher`]; tests use
//! [`test_support::MockFetcher`] (gated behind the `test-support` feature)
//! to inject canned responses without touching the network. Only the
//! binary source kind needs a fetcher — npm uses subprocess, local uses
//! filesystem.

use std::future::Future;
use std::time::Duration;

use crate::InstallError;

/// Hard cap on response body size, enforced while streaming chunks.
///
/// Cheap insurance against a malicious or misconfigured manifest pointing
/// at a multi-gigabyte URL: we refuse rather than OOM. MCP server
/// binaries are typically under 10 MB. If a real first-party manifest
/// blows this cap, revisit the limit (and the streaming-checksum
/// follow-up below).
///
// TODO(chum-v0.2): switch to streaming SHA-256 verification so checksum
// is computed while bytes are received, then we never need to buffer
// more than the chunk size.
pub const MAX_BODY_BYTES: usize = 50 * 1024 * 1024;

/// HTTP fetcher abstraction used by the binary source handler.
///
/// The trait is intentionally narrow — buffer-the-whole-body — because
/// v0.1 favours schema simplicity over memory efficiency. Streaming +
/// checksum-while-streaming can land in v0.2 if any first-party manifest
/// blows the [`MAX_BODY_BYTES`] ceiling.
///
/// The returned future is `+ Send` so this trait composes cleanly with
/// the multi-threaded tokio runtime. Implementors typically write
/// `async fn fetch` and the auto-trait inference picks up Send when the
/// body's awaits are themselves Send.
pub trait Fetcher: Send + Sync {
    /// Fetch the URL and buffer the response body into memory.
    ///
    /// Implementations must:
    /// - Treat any non-2xx status as an error.
    /// - Enforce a body-size cap (production: [`MAX_BODY_BYTES`]).
    /// - Map network / TLS / status errors to
    ///   [`InstallError::FetchFailed`].
    fn fetch(
        &self,
        url: &str,
    ) -> impl Future<Output = Result<Vec<u8>, InstallError>> + Send;
}

/// Production [`Fetcher`] backed by reqwest with rustls-tls.
///
/// Timeouts are locked at v0.1:
///
/// - **Connect:** 30 seconds.
/// - **Total:** 5 minutes.
///
/// No native-tls. Proxies from the environment (`HTTPS_PROXY` etc.) are
/// picked up via reqwest's default builder behaviour.
pub struct ReqwestFetcher {
    client: reqwest::Client,
}

impl ReqwestFetcher {
    /// Construct with the v0.1 default timeouts and a rustls TLS stack.
    ///
    /// # Errors
    ///
    /// [`InstallError::FetchFailed`] if the underlying reqwest client
    /// could not be initialised.
    pub fn new() -> Result<Self, InstallError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| InstallError::FetchFailed {
                url: "<client init>".to_string(),
                source: Box::new(e),
            })?;
        Ok(Self { client })
    }
}

impl Fetcher for ReqwestFetcher {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, InstallError> {
        let mut response =
            self.client
                .get(url)
                .send()
                .await
                .map_err(|e| InstallError::FetchFailed {
                    url: url.to_string(),
                    source: Box::new(e),
                })?;

        let status = response.status();
        if !status.is_success() {
            return Err(InstallError::FetchFailed {
                url: url.to_string(),
                source: format!("HTTP status {status}").into(),
            });
        }

        // Early reject on Content-Length — saves bandwidth if the server
        // is honest about size.
        if let Some(cl) = response.content_length() {
            if cl as usize > MAX_BODY_BYTES {
                return Err(InstallError::FetchFailed {
                    url: url.to_string(),
                    source: format!(
                        "response body content-length {cl} exceeds {MAX_BODY_BYTES}-byte cap"
                    )
                    .into(),
                });
            }
        }

        // Stream chunks so a lying server can't bypass the cap by
        // omitting / fabricating Content-Length.
        let mut buf: Vec<u8> = Vec::new();
        loop {
            let chunk =
                response
                    .chunk()
                    .await
                    .map_err(|e| InstallError::FetchFailed {
                        url: url.to_string(),
                        source: Box::new(e),
                    })?;
            let Some(chunk) = chunk else { break };
            if buf.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(InstallError::FetchFailed {
                    url: url.to_string(),
                    source: format!(
                        "response body exceeds {MAX_BODY_BYTES}-byte cap"
                    )
                    .into(),
                });
            }
            buf.extend_from_slice(&chunk);
        }
        Ok(buf)
    }
}

/// Test helpers. Gated behind the `test-support` cargo feature so
/// production code cannot accidentally depend on this surface.
///
/// To use from integration tests: declare `chum-install` as a
/// dev-dependency with `features = ["test-support"]`, or set
/// `required-features = ["test-support"]` on the `[[test]]` block in
/// the crate's own Cargo.toml.
#[cfg(feature = "test-support")]
pub mod test_support {
    use std::collections::HashMap;
    use std::future::Future;

    use super::Fetcher;
    use crate::InstallError;

    /// Internal storage for a single canned response.
    enum MockResponse {
        Body(Vec<u8>),
        /// Pre-formatted error message; on fetch we wrap it in
        /// `InstallError::FetchFailed` so the wire-shape stays consistent
        /// regardless of how the test author originally classified the
        /// failure.
        Failure(String),
    }

    /// In-memory [`Fetcher`] returning canned bytes (or canned errors)
    /// for canned URLs.
    ///
    /// Construction is single-threaded: each `with_*` call moves `self`
    /// and returns a populated `Self`. Reads from `fetch` see an
    /// immutable map, so no interior mutability is needed.
    pub struct MockFetcher {
        responses: HashMap<String, MockResponse>,
    }

    impl MockFetcher {
        /// Construct an empty mock with no canned responses.
        pub fn new() -> Self {
            Self {
                responses: HashMap::new(),
            }
        }

        /// Register a successful canned response for `url`.
        pub fn with_response(mut self, url: &str, body: Vec<u8>) -> Self {
            self.responses
                .insert(url.to_string(), MockResponse::Body(body));
            self
        }

        /// Register a failure response for `url`.
        ///
        /// The supplied [`InstallError`] is rendered into the
        /// [`InstallError::FetchFailed`] returned by `fetch`. Tests that
        /// need to assert specific variants beyond `FetchFailed` should
        /// trigger those variants higher in the call stack (e.g. by
        /// returning bytes whose checksum will mismatch).
        pub fn with_error(mut self, url: &str, error: InstallError) -> Self {
            self.responses
                .insert(url.to_string(), MockResponse::Failure(error.to_string()));
            self
        }
    }

    impl Default for MockFetcher {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Fetcher for MockFetcher {
        fn fetch(
            &self,
            url: &str,
        ) -> impl Future<Output = Result<Vec<u8>, InstallError>> + Send {
            // Look up synchronously so the returned future doesn't borrow
            // `self.responses` across an await point (it never awaits, but
            // this keeps the signature drop-in compatible with the real
            // fetcher).
            let result = match self.responses.get(url) {
                Some(MockResponse::Body(body)) => Ok(body.clone()),
                Some(MockResponse::Failure(msg)) => Err(InstallError::FetchFailed {
                    url: url.to_string(),
                    source: msg.clone().into(),
                }),
                None => Err(InstallError::FetchFailed {
                    url: url.to_string(),
                    source: "no canned response for URL".into(),
                }),
            };
            async move { result }
        }
    }
}
