use std::{
    future::Future,
    marker::PhantomData,
    num::NonZeroU32,
    pin::Pin,
    sync::Arc,
    task::{ready, Context, Poll},
    time::Duration,
};

use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    response::IntoResponse,
};
use dashmap::DashMap;
use governor::{
    clock::{Clock, DefaultClock, QuantaClock, QuantaInstant},
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use pin_project::pin_project;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tower::{BoxError, Layer, Service};
use tracing::{trace_span, warn};

use crate::layers::util::{
    ExtractionError, KeyExtractor, PeerIpKeyExtractor, SmartIpKeyExtractor, UserIdKeyExtractor,
};

/// Error type returned by rate-limiting layer
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum RateLimitError {
    /// Extraction error
    #[error(transparent)]
    Extraction(#[from] ExtractionError),
    /// Rate limit exceeded
    #[error("Rate limit reached: available after {remaining_seconds} seconds")]
    LimitReached {
        /// Remaining seconds until method becomes available again
        ///
        /// NOTE: Retry-After cannot be specified with fractional digits as per RFC 9110
        remaining_seconds: u64,
    },
}

impl RateLimitError {
    /// HTTP status code for used for this error
    fn http_status(&self) -> StatusCode {
        match self {
            Self::LimitReached { .. } => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoResponse for RateLimitError {
    fn into_response(self) -> Response<Body> {
        problemdetails::new(self.http_status())
            .with_title(self.to_string())
            .into_response()
    }
}

/// Configuration for rate-limiting layer
///
/// Uses [`governor`] crate internally.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct HandlerRateLimitConfig {
    /// Key extractor used to find rate-limiting bucket
    #[serde(default)]
    key: RateLimitKey,
    /// Sustained requests per second
    rps: NonZeroU32,
    /// Maximum requests per second during burst
    #[serde(default, skip_serializing_if = "Option::is_none")]
    burst_rps: Option<NonZeroU32>,
    /// Duration of burst, used for bucket size calculation
    #[serde(
        default = "HandlerRateLimitConfig::default_burst_duration",
        with = "humantime_serde"
    )]
    burst_duration: Duration,
    // TODO: boolean - ignore extraction errors
}

impl HandlerRateLimitConfig {
    /// Default value for [`Self::burst_duration`]
    #[must_use]
    #[inline]
    fn default_burst_duration() -> Duration {
        Duration::from_secs(1)
    }

    /// Helper method to calculate governor burst size
    pub fn burst_size(&self) -> NonZeroU32 {
        let rps = match self.burst_rps {
            Some(rps) if rps > self.rps => rps,
            _ => self.rps,
        };
        let seconds = self.burst_duration.as_secs_f64();
        NonZeroU32::new((seconds * f64::from(rps.get())).ceil() as u32).unwrap_or(self.rps)
    }

    /// Helper method for governor period calculation
    pub fn period(&self) -> Duration {
        Duration::from_secs(1) / self.rps.get()
    }

    /// Create layer for use in [`tower`] services
    pub fn make_layer<S, T>(&self) -> RateLimitLayer<S, T> {
        self.into()
    }
}

/// Method of key extraction for rate limiting
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
enum RateLimitKey {
    /// Global rate limit
    #[default]
    Global,
    /// Per-peer-IP-address rate limit
    PeerIp,
    /// Smart per-peer-IP-address rate limit
    ///
    /// Same as [`RateLimitKey::PeerIp`], but accounts for addresses passed via
    /// `X-Forwarded-For` and similar headers.
    SmartIp,
    /// Per-authenticated-user-ID rate limit
    UserId,
}

/// Rate-limiting [`tower`] layer
pub struct RateLimitLayer<S, T> {
    /// Rate limiter configuration
    config: HandlerRateLimitConfig,
    /// Inner service type
    _phantom_service: PhantomData<S>,
    /// Request body type
    _phantom_request: PhantomData<T>,
}

impl<S, T> From<&HandlerRateLimitConfig> for RateLimitLayer<S, T> {
    fn from(value: &HandlerRateLimitConfig) -> Self {
        // TODO: don't clone, but share, for runtime updates maybe?
        Self {
            config: value.clone(),
            _phantom_service: PhantomData,
            _phantom_request: PhantomData,
        }
    }
}

impl<S, T> Layer<S> for RateLimitLayer<S, T>
where
    S: Service<Request<T>> + Send + 'static,
    T: Send + 'static,
{
    type Service = RateLimit<S, T>;

    fn layer(&self, service: S) -> Self::Service {
        RateLimit::new(service, &self.config)
    }
}

/// Rate-limiting [`tower`] service
pub struct RateLimit<S, T> {
    /// Inner service
    inner: S,
    /// Rate limiter
    limiter: Arc<Box<dyn Limiter<T> + Send + Sync>>,
}

impl<S, T> Clone for RateLimit<S, T>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            limiter: Arc::clone(&self.limiter),
        }
    }
}

impl<S, T> Service<Request<T>> for RateLimit<S, T>
where
    S: Service<Request<T>>,
    S::Error: Into<BoxError>,
{
    type Response = S::Response;
    type Error = BoxError;
    type Future = RateLimitFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.inner.poll_ready(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(res) => Poll::Ready(res.map_err(Into::into)),
        }
    }

    fn call(&mut self, req: Request<T>) -> Self::Future {
        let rate_result = {
            let _span = trace_span!("rate").entered();
            self.limiter.check_limit(&req)
        };
        match rate_result {
            Ok(()) => RateLimitFuture::Positive {
                inner: self.inner.call(req),
            },
            // TODO: option to allow ignoring extraction errors
            Err(error) => {
                if let RateLimitError::LimitReached { remaining_seconds } = &error {
                    warn!(wait = remaining_seconds, "rate limit exceeded");
                }
                RateLimitFuture::Negative { error }
            }
        }
    }
}

impl<S, T> RateLimit<S, T>
where
    S: Service<Request<T>> + Send + 'static,
    T: Send + 'static,
{
    /// Create new rate limiting service
    #[must_use]
    pub fn new(inner: S, config: &HandlerRateLimitConfig) -> Self {
        let limiter: Box<dyn Limiter<T> + Send + Sync> = match config.key {
            RateLimitKey::Global => Box::new(GlobalLimiter::new(config)),
            RateLimitKey::PeerIp => Box::new(KeyedLimiter::new(PeerIpKeyExtractor, config)),
            RateLimitKey::SmartIp => Box::new(KeyedLimiter::new(SmartIpKeyExtractor, config)),
            RateLimitKey::UserId => Box::new(KeyedLimiter::new(UserIdKeyExtractor, config)),
        };
        Self {
            inner,
            limiter: Arc::new(limiter),
        }
    }
}

/// Rate-limiting [`tower`] service future
#[pin_project(project = ProjectedOutcome)]
pub enum RateLimitFuture<F> {
    /// Happy path, calling inner service
    Positive {
        /// Inner future
        #[pin]
        inner: F,
    },
    /// Key extraction error or rate limit exceeded
    Negative {
        /// Cause of negative response
        error: RateLimitError,
    },
}

impl<F, U, E> Future for RateLimitFuture<F>
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

/// Trait for all rate limiters
trait Limiter<T> {
    /// Check whether a request can pass through a rate-limiter
    fn check_limit(&self, req: &Request<T>) -> Result<(), RateLimitError>;
}

/// Global rate limiter
///
/// Does no key extraction from requests.
struct GlobalLimiter {
    limiter: RateLimiter<NotKeyed, InMemoryState, QuantaClock, NoOpMiddleware<QuantaInstant>>,
}

impl<T> Limiter<T> for GlobalLimiter {
    fn check_limit(&self, _req: &Request<T>) -> Result<(), RateLimitError> {
        self.limiter.check().map_err(|neg| {
            let remaining_seconds = neg.wait_time_from(DefaultClock::default().now()).as_secs();
            RateLimitError::LimitReached { remaining_seconds }
        })
    }
}

impl GlobalLimiter {
    /// Create new global limiter
    #[must_use]
    fn new(config: &HandlerRateLimitConfig) -> Self {
        Self {
            limiter: RateLimiter::direct(
                Quota::with_period(config.period())
                    .unwrap()
                    .allow_burst(config.burst_size()),
            ),
        }
    }
}

/// Keyed rate limiter
///
/// Extracts key data from requests using provided [`Self::extractor`].
struct KeyedLimiter<K: KeyExtractor> {
    extractor: K,
    limiters: RateLimiter<
        K::Key,
        DashMap<K::Key, InMemoryState>,
        QuantaClock,
        NoOpMiddleware<QuantaInstant>,
    >,
}

impl<T, K: KeyExtractor> Limiter<T> for KeyedLimiter<K> {
    fn check_limit(&self, req: &Request<T>) -> Result<(), RateLimitError> {
        let key = self.extractor.extract(req)?;
        self.limiters.check_key(&key).map_err(|neg| {
            let remaining_seconds = neg.wait_time_from(DefaultClock::default().now()).as_secs();
            RateLimitError::LimitReached { remaining_seconds }
        })
    }
}

impl<K: KeyExtractor> KeyedLimiter<K> {
    /// Create new keyed limiter
    #[must_use]
    fn new(extractor: K, config: &HandlerRateLimitConfig) -> Self {
        Self {
            extractor,
            limiters: RateLimiter::keyed(
                Quota::with_period(config.period())
                    .unwrap()
                    .allow_burst(config.burst_size()),
            ),
        }
    }
}
