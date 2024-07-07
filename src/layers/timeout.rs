use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use axum::{
    body::Body,
    http::{header::HeaderValue, Request, Response, StatusCode},
    response::IntoResponse,
};
use iso8601_duration::Duration as IsoDuration;
use pin_project::pin_project;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{
    task::futures::TaskLocalFuture,
    time::{sleep_until, Instant, Sleep},
};
use tower::{BoxError, Layer, Service};
use tracing::warn;

use crate::layers::ext::Deadline;

tokio::task_local! {
    /// Deadline of currently executing request, if any
    pub static CURRENT_DEADLINE: Option<Deadline>;
}

/// Error type returned by timeout layer
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum TimeoutError {
    /// Request timed out
    #[error("Request timed out")]
    TimedOut,
}

impl TimeoutError {
    /// HTTP status code for used for this error
    fn http_status(&self) -> StatusCode {
        match self {
            Self::TimedOut => StatusCode::GATEWAY_TIMEOUT,
        }
    }
}

impl IntoResponse for TimeoutError {
    fn into_response(self) -> Response<Body> {
        problemdetails::new(self.http_status())
            .with_title(self.to_string())
            .into_response()
    }
}

/// Handler request timeout configuration
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct HandlerTimeoutConfig {
    /// Allow passing client-supplied ISO8601 timeout duration in an X-Timeout HTTP header
    #[serde(default = "crate::util::default_true")]
    pub use_x_timeout: bool,
    /// Default timeout for a handler
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "timeout",
        with = "humantime_serde"
    )]
    pub default_timeout: Option<Duration>,
    /// Minimum allowed timeout for a method
    ///
    /// Timeout durations less than this value will automatically be responded
    /// with a 504 HTTP status code.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub min_timeout: Option<Duration>,
    /// Maximum allowed timeout for a method
    ///
    /// Timeout durations over this value will automatically be responded
    /// with a 504 HTTP status code.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub max_timeout: Option<Duration>,
}

impl Default for HandlerTimeoutConfig {
    fn default() -> Self {
        Self {
            use_x_timeout: true,
            default_timeout: None,
            min_timeout: None,
            max_timeout: None,
        }
    }
}

impl HandlerTimeoutConfig {
    /// Predicate to skip serializing timeout for [`serde`]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Create layer for use in tower services
    pub fn make_layer<S>(&self) -> Option<TimeoutLayer<S>> {
        if self.use_x_timeout || self.default_timeout.is_some() {
            Some(self.into())
        } else {
            None
        }
    }

    /// Get deadline based on configuration and X-Timeout header
    pub fn get_deadline(&self, timeout_header: Option<&HeaderValue>) -> Option<Instant> {
        if self.use_x_timeout && timeout_header.is_some() {
            timeout_header.and_then(|h| match h.to_str() {
                Ok(s) => match s.parse::<IsoDuration>() {
                    Ok(d) => d.to_std(),
                    Err(error) => {
                        warn!(?error, "unable to parse X-Timeout");
                        self.default_timeout
                    }
                },
                Err(error) => {
                    warn!(%error, "invalid X-Timeout value");
                    self.default_timeout
                }
            })
        } else {
            self.default_timeout
        }
        .and_then(|dur| match self.min_timeout {
            Some(min) if min > dur => {
                warn!(?dur, "duration is shorter than minimum");
                None
            }
            _ => match self.max_timeout {
                Some(max) if max < dur => {
                    warn!(?dur, "duration is longer than maximum");
                    None
                }
                _ => Some(Instant::now() + dur),
            },
        })
    }
}

/// Timeout [`tower`] layer
pub struct TimeoutLayer<S> {
    /// Timeout configuration
    config: HandlerTimeoutConfig,
    /// Inner service type
    _phantom_service: PhantomData<S>,
}

impl<S> From<&HandlerTimeoutConfig> for TimeoutLayer<S> {
    fn from(value: &HandlerTimeoutConfig) -> Self {
        // TODO: don't clone, but share, for runtime updates maybe?
        Self {
            config: value.clone(),
            _phantom_service: PhantomData,
        }
    }
}

impl<S> Layer<S> for TimeoutLayer<S>
where
    S: Service<Request<Body>>,
{
    type Service = TimeoutService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TimeoutService::new(inner, &self.config)
    }
}

/// Timeout [`tower`] service
#[derive(Clone, Debug)]
pub struct TimeoutService<S> {
    /// Timeout configuration
    config: Arc<HandlerTimeoutConfig>,
    /// Inner service
    inner: S,
}

pub(crate) const X_TIMEOUT: &str = "x-timeout";

impl<S> Service<Request<Body>> for TimeoutService<S>
where
    S: Service<Request<Body>>,
    S::Error: Into<BoxError>,
{
    type Response = S::Response;
    type Error = BoxError;
    type Future = TimeoutFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.inner.poll_ready(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(res) => Poll::Ready(res.map_err(Into::into)),
        }
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let deadline = self.config.get_deadline(req.headers().get(X_TIMEOUT));
        let deadline_obj = deadline.map(Deadline::from);
        if let Some(d) = deadline_obj {
            req.extensions_mut().insert(d);
        }
        let inner = CURRENT_DEADLINE.scope(deadline_obj, self.inner.call(req));
        TimeoutFuture::new(inner, deadline)
    }
}

impl<S> TimeoutService<S> {
    /// Create new timeout service
    #[must_use]
    pub fn new(inner: S, config: &HandlerTimeoutConfig) -> Self {
        Self {
            config: Arc::new(config.clone()),
            inner,
        }
    }
}

/// Timeout [`tower`] service future
#[pin_project(project = Type)]
#[derive(Debug)]
pub enum TimeoutFuture<F> {
    /// Timeout exists, enforce it
    Bounded {
        /// Inner future
        #[pin]
        inner: TaskLocalFuture<Option<Deadline>, F>,
        /// Sleep future
        #[pin]
        sleep: Sleep,
    },
    /// Timeout doesn't exist
    Unbounded {
        /// Inner future
        #[pin]
        inner: TaskLocalFuture<Option<Deadline>, F>,
    },
}

impl<F, U, E> Future for TimeoutFuture<F>
where
    F: Future<Output = Result<U, E>>,
    E: Into<BoxError>,
{
    type Output = Result<U, BoxError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            Type::Bounded { inner, sleep } => {
                // Check if future is ready
                match inner.poll(cx) {
                    Poll::Pending => {}
                    Poll::Ready(res) => return Poll::Ready(res.map_err(Into::into)),
                }

                // Inner future is not ready yet, so check the timeout
                match sleep.poll(cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(_) => {
                        warn!("request timed out");
                        Poll::Ready(Err(TimeoutError::TimedOut.into()))
                    }
                }
            }
            Type::Unbounded { inner } => inner.poll(cx).map_err(Into::into),
        }
    }
}

impl<F> TimeoutFuture<F> {
    /// Create new timeout service future
    #[must_use]
    pub fn new(inner: TaskLocalFuture<Option<Deadline>, F>, deadline: Option<Instant>) -> Self {
        match deadline {
            Some(d) => Self::Bounded {
                inner,
                sleep: sleep_until(d),
            },
            None => Self::Unbounded { inner },
        }
    }
}
