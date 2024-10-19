//! [`tower`] layer to record request ID.

use std::{
    marker::PhantomData,
    task::{Context, Poll},
};

use axum::{body::Body, http::Request};
use tokio::task::futures::TaskLocalFuture;
use tower::{Layer, Service};
use tower_http::request_id::RequestId;

tokio::task_local! {
    /// Request ID of currently executing request, if any.
    pub static CURRENT_REQUEST_ID: Option<RequestId>;
}

pub(crate) const X_REQUEST_ID: &str = "x-request-id";

/// Record request ID [`tower`] layer.
#[derive(Clone)]
pub(crate) struct RecordRequestIdLayer<S> {
    /// Inner service type.
    _phantom_service: PhantomData<S>,
}

impl<S> Layer<S> for RecordRequestIdLayer<S>
where
    S: Service<Request<Body>>,
{
    type Service = RecordRequestIdService<S>;

    #[must_use]
    fn layer(&self, inner: S) -> Self::Service {
        RecordRequestIdService::new(inner)
    }
}

impl<S> RecordRequestIdLayer<S> {
    /// Create new record request ID layer.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            _phantom_service: PhantomData,
        }
    }
}

/// Record request ID [`tower`] service.
#[derive(Clone, Debug)]
pub(crate) struct RecordRequestIdService<S> {
    /// Inner service.
    inner: S,
}

impl<S> Service<Request<Body>> for RecordRequestIdService<S>
where
    S: Service<Request<Body>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = TaskLocalFuture<Option<RequestId>, S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let req_id = req.extensions().get::<RequestId>().cloned();
        CURRENT_REQUEST_ID.scope(req_id, self.inner.call(req))
    }
}

impl<S> RecordRequestIdService<S> {
    /// Create new record request ID service.
    #[must_use]
    fn new(inner: S) -> Self {
        Self { inner }
    }
}
