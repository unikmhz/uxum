//! Authentication and authorization [`tower`] layer and service.

use std::{
    future::Future,
    marker::PhantomData,
    ops::Deref,
    pin::Pin,
    sync::Arc,
    task::{ready, Context, Poll},
};

use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    response::IntoResponse,
};
use dyn_clone::clone_box;
use pin_project::pin_project;
use tower::{BoxError, Layer, Service};
use tracing::{trace_span, warn};

use crate::auth::{extractor::AuthExtractor, provider::AuthProvider, user::UserId};

/// Authentication and authorization [`tower`] layer.
#[derive(Clone)]
pub struct AuthLayer<S>(Arc<AuthLayerInner<S>>);

impl<S> Deref for AuthLayer<S> {
    type Target = AuthLayerInner<S>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S> AuthLayer<S> {
    /// Create new [`tower`] auth layer.
    pub fn new(
        permissions: &'static [&'static str],
        auth_provider: Box<dyn AuthProvider>,
        auth_extractor: Box<dyn AuthExtractor>,
    ) -> Self {
        Self(Arc::new(AuthLayerInner {
            permissions,
            auth_provider,
            auth_extractor,
            _phantom_service: PhantomData,
        }))
    }
}

/// Inner struct for [`AuthLayer`].
pub struct AuthLayerInner<S> {
    /// Required permissions for service.
    permissions: &'static [&'static str],
    /// Used auth provider (back-end).
    auth_provider: Box<dyn AuthProvider>,
    /// Used auth extractor (front-end).
    auth_extractor: Box<dyn AuthExtractor>,
    /// Inner service type.
    _phantom_service: PhantomData<S>,
}

impl<S> Clone for AuthLayerInner<S> {
    fn clone(&self) -> Self {
        Self {
            permissions: self.permissions,
            auth_provider: clone_box(self.auth_provider.as_ref()),
            auth_extractor: clone_box(self.auth_extractor.as_ref()),
            _phantom_service: PhantomData,
        }
    }
}

impl<S> Layer<S> for AuthLayer<S> {
    type Service = AuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthService {
            permissions: self.permissions,
            auth_provider: clone_box(self.auth_provider.as_ref()),
            auth_extractor: clone_box(self.auth_extractor.as_ref()),
            inner,
        }
    }
}

/// Authentication and authorization [`tower`] service.
pub struct AuthService<S> {
    /// Required permissions for service.
    permissions: &'static [&'static str],
    /// Used auth provider (back-end).
    auth_provider: Box<dyn AuthProvider>,
    /// Used auth extractor (front-end).
    auth_extractor: Box<dyn AuthExtractor>,
    /// Inner service.
    inner: S,
}

impl<S: Clone> Clone for AuthService<S> {
    fn clone(&self) -> Self {
        Self {
            permissions: self.permissions,
            auth_provider: clone_box(self.auth_provider.as_ref()),
            auth_extractor: clone_box(self.auth_extractor.as_ref()),
            inner: self.inner.clone(),
        }
    }
}

impl<S> Service<Request<Body>> for AuthService<S>
where
    S: Service<Request<Body>, Response = Response<Body>>,
    S::Error: Into<BoxError>,
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
        // Extract user and/or auth token from request.
        let (user, token) = match self.auth_extractor.extract_auth(&req) {
            Ok(pair) => pair,
            Err(error) => {
                warn!(cause = %error, "auth extraction error");
                return AuthFuture::Negative {
                    error_response: Some(self.auth_extractor.error_response(error)),
                };
            }
        };
        // Authenticate user.
        if let Err(error) = self.auth_provider.authenticate(user.as_ref(), &token) {
            warn!(cause = %error, "authentication error");
            return AuthFuture::Negative {
                error_response: Some(self.auth_extractor.error_response(error)),
            };
        }
        // Authorize request.
        for perm in self.permissions {
            if let Err(error) = self.auth_provider.authorize(user.as_ref(), perm) {
                warn!(cause = %error, "authorization error");
                return AuthFuture::Negative {
                    error_response: Some(self.auth_extractor.error_response(error)),
                };
            }
        }
        // Add user ID as an extension into request.
        if let Some(user) = &user {
            req.extensions_mut().insert(user.clone());
        }
        drop(span);
        AuthFuture::Positive {
            inner: self.inner.call(req),
            user_id: user,
        }
    }
}

/// Authentication and authorization [`tower`] service future.
#[pin_project(project = ProjectedOutcome)]
pub enum AuthFuture<F> {
    /// Happy path, calling inner service.
    Positive {
        /// Inner future.
        #[pin]
        inner: F,
        user_id: Option<UserId>,
    },
    /// Authentication error or failure.
    Negative {
        /// Preformatted negative HTTP response.
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
            ProjectedOutcome::Positive { inner, user_id } => {
                let mut resp = ready!(inner.poll(cx).map_err(Into::into))?;
                if let Some(user) = user_id {
                    resp.extensions_mut().insert(user.clone());
                }
                Poll::Ready(Ok(resp))
            }
            ProjectedOutcome::Negative { error_response } => Poll::Ready(Ok(error_response
                .take()
                .unwrap_or_else(|| StatusCode::INTERNAL_SERVER_ERROR.into_response()))),
        }
    }
}
