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

///
pub trait AuthExtractor: Clone + Send {
    ///
    type User: Clone + Send + Sync + 'static;
    ///
    type AuthTokens;

    ///
    fn extract_auth(
        &self,
        req: &Request<Body>,
    ) -> Result<(Self::User, Self::AuthTokens), AuthError>;

    ///
    fn error_response(&self, err: AuthError) -> Response<Body>;
}

///
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

    fn error_response(&self, err: AuthError) -> Response<Body> {
        // This shuld never get executed for a NoOp extractor
        error!("tried to generated auth error response for NoOpAuthExtractor");
        problemdetails::new(StatusCode::INTERNAL_SERVER_ERROR)
            .with_title(err.to_string())
            .into_response()
    }
}

///
#[derive(Clone, Debug, Default)]
pub struct BasicAuthExtractor;

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

    fn error_response(&self, err: AuthError) -> Response<Body> {
        let unauth = matches!(
            err,
            AuthError::NoAuthProvided | AuthError::UserNotFound | AuthError::AuthFailed
        );
        let status = match unauth {
            true => StatusCode::UNAUTHORIZED,
            false => StatusCode::BAD_REQUEST,
        };
        let mut resp = problemdetails::new(status)
            .with_title(err.to_string())
            .into_response();
        if unauth {
            // TODO: add realm, use Self::SCHEME
            let _ = resp.headers_mut().insert(
                WWW_AUTHENTICATE,
                HeaderValue::from_static(r#"Basic charset="UTF-8""#),
            );
        }
        resp
    }
}

impl BasicAuthExtractor {
    ///
    const SCHEME: &'static str = "Basic";

    ///
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

    ///
    fn parse_payload(payload: &str) -> Result<(String, String), AuthError> {
        let raw = String::from_utf8(B64.decode(payload)?)?;
        raw.split_once(':')
            .map(|(user, pwd)| (user.to_string(), pwd.to_string()))
            .ok_or(AuthError::InvalidAuthPayload)
    }
}
