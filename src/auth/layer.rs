use std::{
    borrow::Borrow,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{ready, Context, Poll},
};

use axum::body::Body;
use http::Request;
use pin_project::pin_project;
use tower::{BoxError, Layer, Service};
use tracing::{trace_span, warn};

use crate::auth::{
    errors::AuthError,
    extractor::{AuthExtractor, NoOpAuthExtractor},
    provider::{AuthProvider, NoOpAuthProvider},
};

///
#[derive(Clone)]
pub struct AuthLayer<S, AuthProv = NoOpAuthProvider, AuthExt = NoOpAuthExtractor>
where
    AuthProv: AuthProvider,
    AuthExt: AuthExtractor,
{
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
    S: Service<Request<Body>> + Clone + Send + Sync + 'static,
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
pub struct AuthService<S, AuthProv, AuthExt>
where
    S: Service<Request<Body>> + Clone + Send + Sync + 'static,
    AuthProv: AuthProvider,
    AuthExt: AuthExtractor,
{
    ///
    auth_provider: AuthProv,
    ///
    auth_extractor: AuthExt,
    ///
    inner: S,
}

impl<S, AuthProv, AuthExt> Service<Request<Body>> for AuthService<S, AuthProv, AuthExt>
where
    S: Service<Request<Body>> + Clone + Send + Sync + 'static,
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
        let _span = trace_span!("auth").entered();
        let (user, tokens) = match self.auth_extractor.extract_auth(&req) {
            Ok(pair) => pair,
            Err(error) => {
                warn!(cause = %error, "auth extraction error");
                return AuthFuture::Negative { error }
            }
        };
        if let Err(error) = self
            .auth_provider
            .authenticate(user.borrow(), tokens.borrow())
        {
            warn!(cause = %error, "authentication error");
            return AuthFuture::Negative { error };
        }
        req.extensions_mut().insert(user);
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
        error: AuthError,
    },
}

impl<F, U, E> Future for AuthFuture<F>
where
    F: Future<Output = Result<U, E>>,
    E: Into<BoxError>,
{
    type Output = Result<U, BoxError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            ProjectedOutcome::Positive { inner } => {
                let resp = ready!(inner.poll(cx).map_err(Into::into))?;
                Poll::Ready(Ok(resp))
            }
            ProjectedOutcome::Negative { error } => Poll::Ready(Err(Box::new(error.clone()))),
        }
    }
}
