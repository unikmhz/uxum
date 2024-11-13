//! Misc utility functions and traits.

use std::{
    convert::Infallible,
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{ready, Context, Poll},
};

use axum::{
    http::Request,
    response::{IntoResponse, IntoResponseParts, Response, ResponseParts},
};
use pin_project::pin_project;
use tower::{Layer, Service};

/// Helper function used for default boolean values in [`serde`].
///
/// Always returns `true`.
#[must_use]
#[inline]
pub(crate) fn default_true() -> bool {
    true
}

/// Response layer for adding an extension.
#[derive(Debug, Clone, Copy, Default)]
#[must_use]
#[non_exhaustive]
pub struct ResponseExtension<T>(pub T);

impl<T> Deref for ResponseExtension<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for ResponseExtension<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> IntoResponseParts for ResponseExtension<T>
where
    T: Clone + Send + Sync + 'static,
{
    type Error = Infallible;

    fn into_response_parts(self, mut res: ResponseParts) -> Result<ResponseParts, Self::Error> {
        res.extensions_mut().insert(self.0);
        Ok(res)
    }
}

impl<T> IntoResponse for ResponseExtension<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn into_response(self) -> Response {
        let mut res = ().into_response();
        res.extensions_mut().insert(self.0);
        res
    }
}

impl<S, T> Layer<S> for ResponseExtension<T>
where
    T: Clone + Send + Sync + 'static,
{
    type Service = AddResponseExtension<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        AddResponseExtension {
            inner,
            value: self.0.clone(),
        }
    }
}

/// Middleware for adding extensions to response.
#[derive(Clone, Copy, Debug)]
pub struct AddResponseExtension<S, T> {
    /// Inner service.
    pub(crate) inner: S,
    /// Value to insert as a response extension.
    pub(crate) value: T,
}

impl<B, S, T, U> Service<Request<B>> for AddResponseExtension<S, T>
where
    S: Service<Request<B>, Response = Response<U>>,
    T: Clone + Send + Sync + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseExtensionFuture<S::Future, T>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        ResponseExtensionFuture {
            inner: self.inner.call(req),
            value: self.value.clone(),
        }
    }
}

/// Response future for [`AddResponseExtension`].
#[pin_project]
pub struct ResponseExtensionFuture<F, T> {
    /// Inner future.
    #[pin]
    inner: F,
    /// Value to insert as a response extension.
    value: T,
}

impl<F, T, U, E> Future for ResponseExtensionFuture<F, T>
where
    F: Future<Output = Result<Response<U>, E>>,
    T: Clone + Send + Sync + 'static,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let resp_result = ready!(this.inner.poll(cx));
        Poll::Ready(resp_result.map(|mut resp| {
            resp.extensions_mut().insert(this.value.clone());
            resp
        }))
    }
}
