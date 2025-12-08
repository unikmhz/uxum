//! Trait used to customize application behaviors.

use std::{convert::Infallible, future::Future};

use axum::{
    BoxError,
    body::{Body, HttpBody},
    http::{Request, Response, StatusCode},
    response::IntoResponse,
};
use tower::{Layer, Service, layer::util::Identity};

/// Trait for customizing behaviors within [`crate::AppBuilder`].
pub trait AppBehavior: Clone + Send + Sync + 'static {
    /// Customizable global layer for all handler services.
    fn layer<InSvc, InResp>(
        self,
    ) -> impl Layer<
        InSvc,
        Service = impl Service<
            Request<Body>,
            Response = Response<impl HttpBody<Data = bytes::Bytes, Error = BoxError> + Send>,
            Error = Infallible,
            Future = impl Send,
        > + Clone
                  + Send
                  + Sync,
    > + Clone
    + Send
    + Sync
    where
        InSvc: Service<Request<Body>, Response = Response<InResp>, Error = Infallible>
            + Clone
            + Send
            + Sync,
        InSvc::Future: Send,
        InResp: HttpBody<Data = bytes::Bytes, Error = BoxError> + Send,
        InResp::Data: Send,
    {
        // XXX: consider moving to `BoxLayer` in future versions.
        Identity::new()
    }

    /// Customizable code for readiness probe.
    fn readiness_probe(&self) -> impl Future<Output = impl IntoResponse> + Send {
        async { StatusCode::OK }
    }

    /// Customizable code for liveness probe.
    fn liveness_probe(&self) -> impl Future<Output = impl IntoResponse> + Send {
        async { StatusCode::OK }
    }
}

/// Standard application behavior, used by default.
#[derive(Clone)]
pub struct StandardAppBehavior;

impl AppBehavior for StandardAppBehavior {}
