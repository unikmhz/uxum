//! AAA - extractors.

#[cfg(feature = "jwt")]
use std::collections::HashMap;
use std::{borrow::Cow, collections::BTreeMap};

use axum::{
    body::Body,
    http::{
        header::{AUTHORIZATION, WWW_AUTHENTICATE},
        HeaderValue, Request, Response, StatusCode,
    },
    response::IntoResponse,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
#[cfg(feature = "jwt")]
use deboog::Deboog;
use dyn_clone::DynClone;
#[cfg(feature = "jwt")]
use jsonwebtoken as jwt;
use okapi::{openapi3, Map};
#[cfg(feature = "jwt")]
use serde_json::Value;
use tracing::error;

use crate::{
    auth::{errors::AuthError, token::AuthToken, user::UserId},
    errors,
};

/// Authentication extractor (front-end) trait.
pub trait AuthExtractor: std::fmt::Debug + DynClone + Send + Sync + 'static {
    /// Extract user ID and authentication data from request.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any preconditions for auth data extraction have not been met.
    fn extract_auth(&self, req: &Request<Body>) -> Result<(Option<UserId>, AuthToken), AuthError>;

    /// Format error response from [`AuthError`].
    ///
    /// Passed to auth provider (back-end) for authentication and authorization.
    #[must_use]
    fn error_response(&self, err: AuthError) -> Response<Body>;

    /// Get schema objects corresponding to authentication methods.
    #[must_use]
    fn security_schemes(&self) -> BTreeMap<String, openapi3::SecurityScheme> {
        BTreeMap::new()
    }
}

/// Authentication extractor (front-end) which does nothing.
#[derive(Clone, Debug, Default)]
pub struct NoOpAuthExtractor;

impl AuthExtractor for NoOpAuthExtractor {
    fn extract_auth(&self, _req: &Request<Body>) -> Result<(Option<UserId>, AuthToken), AuthError> {
        Ok((None, AuthToken::Absent))
    }

    fn error_response(&self, err: AuthError) -> Response<Body> {
        // This shuld never get executed for a NoOp extractor
        error!("tried to generate auth error response for NoOpAuthExtractor");
        problemdetails::new(StatusCode::INTERNAL_SERVER_ERROR)
            .with_type(errors::TAG_UXUM_AUTH)
            .with_title(err.to_string())
            .into_response()
    }
}

/// Authentication extractor (front-end) for HTTP Basic authentication.
#[derive(Clone, Debug)]
pub struct BasicAuthExtractor {
    /// Value to use for `WWW-Authenticate` header.
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
    fn extract_auth(&self, req: &Request<Body>) -> Result<(Option<UserId>, AuthToken), AuthError> {
        match req.headers().get(AUTHORIZATION) {
            Some(header) => {
                Self::parse_header(header).map(|(user, pwd)| (Some(user.into()), pwd.into()))
            }
            None => Err(AuthError::NoAuthProvided),
        }
    }

    fn error_response(&self, err: AuthError) -> Response<Body> {
        let status = match err {
            AuthError::NoAuthProvided | AuthError::UserNotFound | AuthError::AuthFailed => {
                StatusCode::UNAUTHORIZED
            }
            AuthError::NoPermission(_) => StatusCode::FORBIDDEN,
            _ => StatusCode::BAD_REQUEST,
        };
        let mut resp = problemdetails::new(status)
            .with_type(errors::TAG_UXUM_AUTH)
            .with_title(err.to_string())
            .into_response();
        if status == StatusCode::UNAUTHORIZED {
            let header_value = match HeaderValue::from_str(&self.www_auth) {
                Ok(val) => val,
                Err(err) => {
                    return problemdetails::new(StatusCode::INTERNAL_SERVER_ERROR)
                        .with_type(errors::TAG_UXUM_AUTH)
                        .with_title("Invalid HTTP Basic realm value")
                        .with_detail(err.to_string())
                        .into_response()
                }
            };
            let _ = resp.headers_mut().insert(WWW_AUTHENTICATE, header_value);
        }
        resp
    }

    fn security_schemes(&self) -> BTreeMap<String, openapi3::SecurityScheme> {
        maplit::btreemap! {
            "basic".into() => openapi3::SecurityScheme {
                description: Some("HTTP Basic authentication".into()),
                data: openapi3::SecuritySchemeData::Http {
                    scheme: "basic".into(),
                    bearer_format: None,
                },
                extensions: Map::default(),
            },
        }
    }
}

impl BasicAuthExtractor {
    /// Name of authentication scheme.
    const SCHEME: &'static str = "Basic";

    /// Create new extractor, passing optional parameters.
    pub fn new(realm: Option<impl AsRef<str>>) -> Self {
        match realm {
            Some(realm) => Self {
                www_auth: Cow::Owned(Self::format_www_authenticate(realm)),
            },
            None => Default::default(),
        }
    }

    /// Format value of `WWW-Authenticate` header.
    fn format_www_authenticate(realm: impl AsRef<str>) -> String {
        // TODO: escape realm
        format!(
            r#"{} realm="{}", charset="UTF-8""#,
            Self::SCHEME,
            realm.as_ref()
        )
    }

    /// Parse `Authorization` header into plaintext username and password.
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

    /// Parse base64-encoded credentials into plaintext username and password.
    fn parse_payload(payload: &str) -> Result<(String, String), AuthError> {
        let raw = String::from_utf8(B64.decode(payload)?)?;
        raw.split_once(':')
            .map(|(user, pwd)| (user.to_string(), pwd.to_string()))
            .ok_or(AuthError::InvalidAuthPayload)
    }

    /// Set realm used for HTTP authentication challenge.
    pub fn set_realm(&mut self, realm: impl AsRef<str>) {
        self.www_auth = Cow::Owned(Self::format_www_authenticate(realm));
    }
}

/// Authentication extractor (front-end) that gets user and password from HTTP headers.
#[derive(Clone, Debug)]
pub struct HeaderAuthExtractor {
    /// Header name for user identifier.
    ///
    /// Default is "X-API-Name".
    user_header: Cow<'static, str>,
    /// Header name for user authentication info.
    ///
    /// Default is "X-API-Key".
    token_header: Cow<'static, str>,
}

impl Default for HeaderAuthExtractor {
    fn default() -> Self {
        Self {
            user_header: Cow::Borrowed("X-API-Name"),
            token_header: Cow::Borrowed("X-API-Key"),
        }
    }
}

impl AuthExtractor for HeaderAuthExtractor {
    fn extract_auth(&self, req: &Request<Body>) -> Result<(Option<UserId>, AuthToken), AuthError> {
        let headers = req.headers();
        let user = match headers.get(self.user_header.as_ref()) {
            Some(header) => match header.to_str() {
                Ok(user) => user.into(),
                Err(_) => return Err(AuthError::InvalidAuthPayload),
            },
            None => return Err(AuthError::NoAuthProvided),
        };
        let token = match headers.get(self.token_header.as_ref()) {
            Some(header) => match header.to_str() {
                Ok(user) => user.to_string(),
                Err(_) => return Err(AuthError::InvalidAuthPayload),
            },
            None => return Err(AuthError::NoAuthProvided),
        };
        Ok((Some(user), token.into()))
    }

    fn error_response(&self, err: AuthError) -> Response<Body> {
        let status = match err {
            AuthError::NoAuthProvided
            | AuthError::UserNotFound
            | AuthError::AuthFailed
            | AuthError::NoPermission(_) => StatusCode::FORBIDDEN,
            _ => StatusCode::BAD_REQUEST,
        };
        problemdetails::new(status)
            .with_type(errors::TAG_UXUM_AUTH)
            .with_title(err.to_string())
            .into_response()
    }

    fn security_schemes(&self) -> BTreeMap<String, openapi3::SecurityScheme> {
        maplit::btreemap! {
            "api-name".into() => openapi3::SecurityScheme {
                description: Some("API user name".into()),
                data: openapi3::SecuritySchemeData::ApiKey {
                    name: self.user_header.to_string(),
                    location: "header".into(),
                },
                extensions: Map::default(),
            },
            "api-key".into() => openapi3::SecurityScheme {
                description: Some("API key".into()),
                data: openapi3::SecuritySchemeData::ApiKey {
                    name: self.token_header.to_string(),
                    location: "header".into(),
                },
                extensions: Map::default(),
            },
        }
    }
}

impl HeaderAuthExtractor {
    /// Create new extractor, passing optional parameters.
    pub fn new(
        user_header: Option<impl AsRef<str>>,
        token_header: Option<impl AsRef<str>>,
    ) -> Self {
        let mut extractor = Self::default();
        if let Some(header) = user_header {
            extractor.user_header = Cow::Owned(header.as_ref().to_string());
        }
        if let Some(header) = token_header {
            extractor.token_header = Cow::Owned(header.as_ref().to_string());
        }
        extractor
    }

    /// Set user ID header name.
    pub fn set_user_header(&mut self, name: impl AsRef<str>) {
        self.user_header = Cow::Owned(name.as_ref().into());
    }

    /// Set authenticating token header name.
    pub fn set_token_header(&mut self, name: impl AsRef<str>) {
        self.token_header = Cow::Owned(name.as_ref().into());
    }
}

/// Authentication extractor (front-end) for HTTP Bearer authentication using signed JWT.
#[cfg(feature = "jwt")]
#[derive(Clone, Deboog)]
pub struct JwtAuthExtractor {
    /// Decoding key.
    #[deboog(skip)]
    key: jwt::DecodingKey,
    /// JWT validation configuration.
    validation: jwt::Validation,
    /// Value to use for `WWW-Authenticate` header.
    ///
    /// Default value uses "auth" string as a realm.
    www_auth: Cow<'static, str>,
}

#[cfg(feature = "jwt")]
impl AuthExtractor for JwtAuthExtractor {
    fn extract_auth(&self, req: &Request<Body>) -> Result<(Option<UserId>, AuthToken), AuthError> {
        match req.headers().get(AUTHORIZATION) {
            Some(header) => {
                let claims = self.parse_header(header)?;
                // TODO: customize user ID location inside claims.
                // TODO: parse user ID from formatted field, stripping out prefixes/suffixes.
                let user = claims.get("sub").and_then(|v| match v {
                    Value::String(s) => Some(s.as_str().into()),
                    Value::Number(n) => Some(n.to_string().into()),
                    _ => None,
                });
                Ok((user, AuthToken::ExternallyVerified))
            }
            None => Err(AuthError::NoAuthProvided),
        }
    }

    fn error_response(&self, err: AuthError) -> Response<Body> {
        let status = match err {
            AuthError::NoAuthProvided | AuthError::UserNotFound | AuthError::AuthFailed => {
                StatusCode::UNAUTHORIZED
            }
            AuthError::NoPermission(_) => StatusCode::FORBIDDEN,
            _ => StatusCode::BAD_REQUEST,
        };
        let mut resp = problemdetails::new(status)
            .with_type(errors::TAG_UXUM_AUTH)
            .with_title(err.to_string())
            .into_response();
        if status == StatusCode::UNAUTHORIZED {
            let header_value = match HeaderValue::from_str(&self.www_auth) {
                Ok(val) => val,
                Err(err) => {
                    return problemdetails::new(StatusCode::INTERNAL_SERVER_ERROR)
                        .with_type(errors::TAG_UXUM_AUTH)
                        .with_title("Invalid HTTP Basic realm value")
                        .with_detail(err.to_string())
                        .into_response()
                }
            };
            let _ = resp.headers_mut().insert(WWW_AUTHENTICATE, header_value);
        }
        resp
    }

    fn security_schemes(&self) -> BTreeMap<String, openapi3::SecurityScheme> {
        maplit::btreemap! {
            "bearer".into() => openapi3::SecurityScheme {
                description: Some("HTTP Bearer authentication".into()),
                data: openapi3::SecuritySchemeData::Http {
                    scheme: "bearer".into(),
                    bearer_format: Some("JWT".into()),
                },
                extensions: Map::default(),
            },
        }
    }
}

#[cfg(feature = "jwt")]
impl JwtAuthExtractor {
    /// Name of authentication scheme.
    const SCHEME: &'static str = "Bearer";

    /// Create new extractor.
    pub fn new(
        realm: Option<impl AsRef<str>>,
        key: jwt::DecodingKey,
        validation: jwt::Validation,
    ) -> Self {
        let www_auth = match realm {
            Some(realm) => Cow::Owned(Self::format_www_authenticate(realm)),
            None => Cow::Borrowed(r#"Bearer realm="auth", charset="UTF-8""#),
        };
        Self {
            key,
            validation,
            www_auth,
        }
    }

    /// Format value of `WWW-Authenticate` header.
    fn format_www_authenticate(realm: impl AsRef<str>) -> String {
        // TODO: escape realm
        format!(
            r#"{} realm="{}", charset="UTF-8""#,
            Self::SCHEME,
            realm.as_ref()
        )
    }

    /// Parse `Authorization` header into token value.
    fn parse_header(&self, header: &HeaderValue) -> Result<HashMap<String, Value>, AuthError> {
        let Ok(header) = header.to_str() else {
            return Err(AuthError::InvalidAuthHeader);
        };
        match header.split_once(' ') {
            Some((scheme, payload)) if scheme.eq_ignore_ascii_case(Self::SCHEME) => {
                self.parse_payload(payload)
            }
            Some((scheme, _)) => Err(AuthError::UnknownAuthScheme(scheme.to_string())),
            None => Err(AuthError::InvalidAuthHeader),
        }
    }

    /// Parse base64-encoded credentials into plaintext username and password.
    fn parse_payload(&self, payload: &str) -> Result<HashMap<String, Value>, AuthError> {
        use jwt::errors::ErrorKind;
        // TODO: maybe use spawn_blocking? crypto can be resource-intensive.
        match jwt::decode(payload, &self.key, &self.validation) {
            Ok(token) => Ok(token.claims),
            // TODO: maybe add more AuthError variants?
            Err(err) => match err.kind() {
                ErrorKind::InvalidSignature => Err(AuthError::AuthFailed),
                ErrorKind::ExpiredSignature => Err(AuthError::AuthFailed),
                _ => Err(AuthError::InvalidAuthPayload),
            },
        }
    }

    /// Set realm used for HTTP authentication challenge.
    pub fn set_realm(&mut self, realm: impl AsRef<str>) {
        self.www_auth = Cow::Owned(Self::format_www_authenticate(realm));
    }

    /// Set JWT decoding key.
    ///
    /// See [`jsonwebtoken::DecodingKey`].
    pub fn set_key(&mut self, key: jwt::DecodingKey) {
        self.key = key;
    }

    /// Set JWT validation parameters.
    ///
    /// See [`jsonwebtoken::Validation`].
    pub fn set_validation(&mut self, valid: jwt::Validation) {
        self.validation = valid;
    }
}
