//! HTTP client - error type.

use thiserror::Error;

use crate::errors::IoError;

/// Error type used in HTTP client subsystem.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HttpClientError {
    /// HTTP client error.
    #[error("HTTP client error: {0}")]
    Reqwest(#[from] reqwest::Error),
    /// Error loading TLS identity.
    #[error("Error loading TLS identity: {0}")]
    IdentityLoad(IoError),
}

impl HttpClientError {
    /// Generate new [`HttpClientError::IdentityLoad`] error.
    pub fn identity_load(err: impl Into<IoError>) -> Self {
        Self::IdentityLoad(err.into())
    }
}
