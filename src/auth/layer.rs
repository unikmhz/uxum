use std::{
    borrow::Borrow,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{ready, Context, Poll},
};

use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    response::IntoResponse,
};
use pin_project::pin_project;
use tower::{BoxError, Layer, Service};
use tracing::{trace_span, warn};

use crate::auth::{
    extractor::{AuthExtractor, NoOpAuthExtractor},
    provider::{AuthProvider, NoOpAuthProvider},
};

///
#[derive(Clone)]
pub struct AuthLayer<S, AuthProv = NoOpAuthProvider, AuthExt = NoOpAuthExtractor> {
    ///
    auth_provider: AuthProv,
    ///
    auth_extractor: AuthExt,
    ///
    _phantom_service: PhantomData<S>,
}

impl<S, AuthProv, AuthExt> AuthLayer<S, AuthProv, AuthExt>
where
    AuthProv: AuthProvider,
    AuthExt: AuthExtractor,
{
    ///
    pub fn new(auth_provider: AuthProv, auth_extractor: AuthExt) -> Self {
        Self {
            auth_provider,
            auth_extractor,
            _phantom_service: PhantomData,
        }
    }
}

impl<S, AuthProv, AuthExt> Layer<S> for AuthLayer<S, AuthProv, AuthExt>
where
    AuthProv: AuthProvider,
    AuthExt: AuthExtractor,
{
    type Service = AuthService<S, AuthProv, AuthExt>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthService {
            auth_provider: self.auth_provider.clone(),
            auth_extractor: self.auth_extractor.clone(),
            inner,
        }
    }
}

/// Authenticating [`tower`] layer
#[derive(Clone)]
pub struct AuthService<S, AuthProv, AuthExt> {
    ///
    auth_provider: AuthProv,
    ///
    auth_extractor: AuthExt,
    ///
    inner: S,
}

impl<S, AuthProv, AuthExt> Service<Request<Body>> for AuthService<S, AuthProv, AuthExt>
where
    S: Service<Request<Body>, Response = Response<Body>>,
    S::Error: Into<BoxError>,
    AuthProv: AuthProvider,
    AuthExt: AuthExtractor,
    AuthExt::User: Borrow<AuthProv::User> + Clone + Send + Sync + 'static,
    AuthExt::AuthTokens: Borrow<AuthProv::AuthTokens>,
{
    type Response = S::Response;
    type Error = BoxError;
    type Future = AuthFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.inner.poll_ready(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(res) => Poll::Ready(res.map_err(Into::into)),
        }
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let span = trace_span!("auth").entered();
        let (user, tokens) = match self.auth_extractor.extract_auth(&req) {
            Ok(pair) => pair,
            Err(error) => {
                warn!(cause = %error, "auth extraction error");
                return AuthFuture::Negative {
                    error_response: Some(self.auth_extractor.error_response(error)),
                };
            }
        };
        if let Err(error) = self
            .auth_provider
            .authenticate(user.borrow(), tokens.borrow())
        {
            warn!(cause = %error, "authentication error");
            return AuthFuture::Negative {
                error_response: Some(self.auth_extractor.error_response(error)),
            };
        }
        req.extensions_mut().insert(user);
        drop(span);
        AuthFuture::Positive {
            inner: self.inner.call(req),
        }
    }
}

///
#[pin_project(project = ProjectedOutcome)]
pub enum AuthFuture<F> {
    Positive {
        #[pin]
        inner: F,
    },
    Negative {
        error_response: Option<Response<Body>>,
    },
}

impl<F, E> Future for AuthFuture<F>
where
    F: Future<Output = Result<Response<Body>, E>>,
    E: Into<BoxError>,
{
    type Output = Result<Response<Body>, BoxError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            ProjectedOutcome::Positive { inner } => {
                let resp = ready!(inner.poll(cx).map_err(Into::into))?;
                Poll::Ready(Ok(resp))
            }
            ProjectedOutcome::Negative { error_response } => Poll::Ready(Ok(error_response
                .take()
                .unwrap_or_else(|| StatusCode::INTERNAL_SERVER_ERROR.into_response()))),
        }
    }
}
