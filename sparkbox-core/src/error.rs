//! Error type shared by the whole core.

/// Every fallible core operation returns this.
///
/// `Api` mirrors the plain-string errors the original Dart client threw
/// (e.g. `h5 HTTP 400: ...`, `No streams (se=1 ep=2) | ...`), so error text
/// shown in the app stays familiar.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// Upstream/protocol error with a human-readable message.
    #[error("{0}")]
    Api(String),

    /// Transport failure (DNS, connect, timeout, TLS...).
    #[error(transparent)]
    Http(#[from] reqwest::Error),

    /// Response body was not the JSON we expected.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Convenience alias used across the crate.
pub type Result<T> = std::result::Result<T, CoreError>;
