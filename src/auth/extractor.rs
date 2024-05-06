use std::borrow::Cow;

use axum::{
    body::Body,
    http::{
        header::{AUTHORIZATION, WWW_AUTHENTICATE},
        HeaderValue, Request, Response, StatusCode,
    },
    response::IntoResponse,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use tracing::error;

use crate::auth::{errors::AuthError, user::UserId};

/// Authentication extractor (front-end) trait
pub trait AuthExtractor: Clone + Send {
    /// User ID type
    ///
    /// Passed to auth provider (back-end) for authentication and authorization.
    /// On successful authentication it is injected into request as an extension.
    type User: Clone + Send + Sync + 'static;
    /// Authentication data type
    type AuthTokens;

    /// Extract user ID and authentication data from request
    fn extract_auth(
        &self,
        req: &Request<Body>,
    ) -> Result<(Self::User, Self::AuthTokens), AuthError>;

    /// Format error response from [`AuthError`]
    ///
    /// Passed to auth provider (back-end) for authentication and authorization.
    #[must_use]
    fn error_response(&self, err: AuthError) -> Response<Body>;
}

/// Authentication extractor (front-end) which does nothing
#[derive(Clone, Debug, Default)]
pub struct NoOpAuthExtractor;

impl AuthExtractor for NoOpAuthExtractor {
    type User = ();
    type AuthTokens = ();

    fn extract_auth(
        &self,
        _req: &Request<Body>,
    ) -> Result<(Self::User, Self::AuthTokens), AuthError> {
        Ok(((), ()))
    }

    #[must_use]
    fn error_response(&self, err: AuthError) -> Response<Body> {
        // This shuld never get executed for a NoOp extractor
        error!("tried to generated auth error response for NoOpAuthExtractor");
        problemdetails::new(StatusCode::INTERNAL_SERVER_ERROR)
            .with_title(err.to_string())
            .into_response()
    }
}

/// Authentication extractor (front-end) for HTTP Basic authentication
#[derive(Clone, Debug)]
pub struct BasicAuthExtractor {
    /// Value to use for `WWW-Authenticate` header
    ///
    /// Default value uses "auth" string as a realm.
    www_auth: Cow<'static, str>,
}

impl Default for BasicAuthExtractor {
    fn default() -> Self {
        Self {
            www_auth: Cow::Borrowed(r#"Basic realm="auth", charset="UTF-8""#),
        }
    }
}

impl AuthExtractor for BasicAuthExtractor {
    type User = UserId;
    type AuthTokens = String;

    fn extract_auth(
        &self,
        req: &Request<Body>,
    ) -> Result<(Self::User, Self::AuthTokens), AuthError> {
        match req.headers().get(AUTHORIZATION) {
            Some(header) => Self::parse_header(header).map(|(user, pwd)| (user.into(), pwd)),
            None => Err(AuthError::NoAuthProvided),
        }
    }

    #[must_use]
    fn error_response(&self, err: AuthError) -> Response<Body> {
        let status = match err {
            AuthError::NoAuthProvided | AuthError::UserNotFound | AuthError::AuthFailed => {
                StatusCode::UNAUTHORIZED
            }
            AuthError::NoPermission(_) => StatusCode::FORBIDDEN,
            _ => StatusCode::BAD_REQUEST,
        };
        let mut resp = problemdetails::new(status)
            .with_title(err.to_string())
            .into_response();
        if status == StatusCode::UNAUTHORIZED {
            let header_value = match HeaderValue::from_str(&self.www_auth) {
                Ok(val) => val,
                Err(err) => {
                    return problemdetails::new(StatusCode::INTERNAL_SERVER_ERROR)
                        .with_title("Invalid HTTP Basic realm value")
                        .with_detail(err.to_string())
                        .into_response()
                }
            };
            let _ = resp.headers_mut().insert(WWW_AUTHENTICATE, header_value);
        }
        resp
    }
}

impl BasicAuthExtractor {
    /// Name of authentication scheme
    const SCHEME: &'static str = "Basic";

    /// Format value of `WWW-Authenticate` header
    fn format_www_authenticate(&self, realm: impl AsRef<str>) -> String {
        // TODO: escape realm
        format!(
            r#"{} realm="{}", charset="UTF-8""#,
            Self::SCHEME,
            realm.as_ref()
        )
    }

    /// Parse `Authorization` header into plaintext username and password
    fn parse_header(header: &HeaderValue) -> Result<(String, String), AuthError> {
        let Ok(header) = header.to_str() else {
            return Err(AuthError::InvalidAuthHeader);
        };
        match header.split_once(' ') {
            Some((scheme, payload)) if scheme.eq_ignore_ascii_case(Self::SCHEME) => {
                Self::parse_payload(payload)
            }
            Some((scheme, _)) => Err(AuthError::UnknownAuthScheme(scheme.to_string())),
            None => Err(AuthError::InvalidAuthHeader),
        }
    }

    /// Parse base64-encoded credentials into plaintext username and password
    fn parse_payload(payload: &str) -> Result<(String, String), AuthError> {
        let raw = String::from_utf8(B64.decode(payload)?)?;
        raw.split_once(':')
            .map(|(user, pwd)| (user.to_string(), pwd.to_string()))
            .ok_or(AuthError::InvalidAuthPayload)
    }

    /// Set realm used for HTTP authentication challenge
    pub fn set_realm(&mut self, realm: impl AsRef<str>) {
        self.www_auth = Cow::Owned(self.format_www_authenticate(realm));
    }
}
