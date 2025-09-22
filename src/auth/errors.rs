//! AAA - errors.

use thiserror::Error;

use crate::errors::IoError;

/// Error type used in authentication and authorization layer.
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum AuthError {
    /// No authentication data provided.
    #[error("No authentication data provided")]
    NoAuthProvided,
    /// Invalid format for Authorization header.
    #[error("Invalid format for Authorization header")]
    InvalidAuthHeader,
    /// Unknown authentication scheme.
    #[error("Unknown authentication scheme: {0}")]
    UnknownAuthScheme(String),
    /// Authentication payload decoding error.
    #[error("Authentication payload decoding error: {0}")]
    PayloadDecode(#[from] base64::DecodeError),
    /// Authentication payload is not printable.
    #[error("Authentication payload is not printable: {0}")]
    NonPrintablePayload(#[from] std::string::FromUtf8Error),
    /// Invalid authentication payload.
    #[error("Invalid authentication payload")]
    InvalidAuthPayload,
    /// User not recognized.
    ///
    /// [`AuthError::UserNotFound`] and [`AuthError::AuthFailed`] must produce exactly the same
    /// response to not divulge sensitive information.
    #[error("Authentication failed")]
    UserNotFound,
    /// Authentication failed.
    ///
    /// [`AuthError::UserNotFound`] and [`AuthError::AuthFailed`] must produce exactly the same
    /// response to not divulge sensitive information.
    #[error("Authentication failed")]
    AuthFailed,
    /// User does not have permission.
    #[error("User does not have permission: {0}")]
    NoPermission(&'static str),
}

/// Error on setting up authentication framework.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AuthSetupError {
    /// Key loading error.
    #[error("Key loading error (file {0}): {1}")]
    KeyIo(String, IoError),
    #[cfg(feature = "jwt")]
    /// Key decoding error.
    #[error("Key decoding error: {0}")]
    KeyDecode(#[from] jsonwebtoken::errors::Error),
}
