use axum::{
    body::Body,
    http::{Response, StatusCode},
    response::IntoResponse,
};
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
}

impl AuthError {
    fn http_status(&self) -> StatusCode {
        match self {
            Self::NoAuthProvided => StatusCode::UNAUTHORIZED,
            Self::UserNotFound => StatusCode::UNAUTHORIZED,
            Self::AuthFailed => StatusCode::UNAUTHORIZED,
            _ => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response<Body> {
        // TODO: add WWW-Authenticate
        problemdetails::new(self.http_status())
            .with_title(self.to_string())
            .into_response()
    }
}
