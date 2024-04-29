use thiserror::Error;

///
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum AuthError {
    ///
    #[error("No authentication data provided")]
    NoAuthProvided,
    ///
    #[error("Invalid format for Authorization header")]
    InvalidAuthHeader,
    ///
    #[error("Unknown authentication scheme: {0}")]
    UnknownAuthScheme(String),
    ///
    #[error("Authentication payload decoding error: {0}")]
    PayloadDecode(#[from] base64::DecodeError),
    ///
    #[error("Authentication payload is not printable: {0}")]
    NonPrintablePayload(#[from] std::string::FromUtf8Error),
    ///
    #[error("Invalid authentication payload")]
    InvalidAuthPayload,
    ///
    #[error("User not recognized")]
    UserNotFound,
    ///
    #[error("Authentication failed")]
    AuthFailed,
    ///
    #[error("User does not have permission: {0}")]
    NoPermission(&'static str),
}
