use std::{
    future::Future,
    hash::Hash,
    marker::PhantomData,
    net::{IpAddr, SocketAddr},
    num::NonZeroU32,
    pin::Pin,
    sync::Arc,
    task::{ready, Context, Poll},
    time::Duration,
};

use axum::extract::ConnectInfo;
use dashmap::DashMap;
use forwarded_header_value::{ForwardedHeaderValue, Identifier};
use governor::{
    clock::{Clock, DefaultClock, QuantaClock, QuantaInstant},
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use hyper::{header::FORWARDED, HeaderMap, Request};
use pin_project::pin_project;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tower::{BoxError, Layer, Service};
use tracing::warn;

/// Error type returned by rate-limiting layer
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum RateLimitError {
    #[error("Unable to extract rate-limiting key from request")]
    ExtractionError,
    #[error("Rate limit reached: available after {remaining_seconds} seconds")]
    LimitReached {
        // NOTE: Retry-After cannot be specified with fractional digits as per RFC 9110
        remaining_seconds: u64,
    },
}

/// Configuration for rate-limiting layer
///
/// Uses [`governor`] crate internally.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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

    /// Create layer for use in tower services
    pub fn make_layer<S, T>(&self) -> RateLimitLayer<S, T> {
        self.into()
    }
}

/// Method of key extraction for rate limiting
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
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
}

/// Rate-limiting [`tower`] layer factory
pub struct RateLimitLayer<S, T> {
    config: HandlerRateLimitConfig,
    _phantom_service: PhantomData<S>,
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
    S: Service<Request<T>> + Send + Sync + 'static,
    T: Send + 'static,
{
    type Service = RateLimit<S, T>;

    fn layer(&self, service: S) -> Self::Service {
        RateLimit::new(service, &self.config)
    }
}

/// Rate-limiting [`tower`] layer
pub struct RateLimit<S, T> {
    inner: S,
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
        match self.limiter.check_limit(&req) {
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
    S: Service<Request<T>> + Send + Sync + 'static,
    T: Send + 'static,
{
    #[must_use]
    pub fn new(inner: S, config: &HandlerRateLimitConfig) -> Self {
        let limiter: Box<dyn Limiter<T> + Send + Sync> = match config.key {
            RateLimitKey::Global => Box::new(GlobalLimiter::new(config)),
            RateLimitKey::PeerIp => Box::new(IpLimiter::new(PeerIpKeyExtractor, config)),
            RateLimitKey::SmartIp => Box::new(IpLimiter::new(SmartIpKeyExtractor, config)),
        };
        Self {
            inner,
            limiter: Arc::new(limiter),
        }
    }
}

#[pin_project(project = ProjectedOutcome)]
pub enum RateLimitFuture<F> {
    Positive {
        #[pin]
        inner: F,
    },
    Negative {
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
    ///
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

/// Per-IP rate limiter
///
/// Extracts key data from requests using provided [`Self::extractor`].
struct IpLimiter<K: KeyExtractor> {
    extractor: K,
    limiters: RateLimiter<
        K::Key,
        DashMap<K::Key, InMemoryState>,
        QuantaClock,
        NoOpMiddleware<QuantaInstant>,
    >,
}

impl<T, K: KeyExtractor> Limiter<T> for IpLimiter<K> {
    ///
    fn check_limit(&self, req: &Request<T>) -> Result<(), RateLimitError> {
        let key = self.extractor.extract(req)?;
        self.limiters.check_key(&key).map_err(|neg| {
            let remaining_seconds = neg.wait_time_from(DefaultClock::default().now()).as_secs();
            RateLimitError::LimitReached { remaining_seconds }
        })
    }
}

impl<K: KeyExtractor> IpLimiter<K> {
    ///
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

trait KeyExtractor {
    type Key: Hash + Eq + Clone;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, RateLimitError>;
}

struct PeerIpKeyExtractor;

impl KeyExtractor for PeerIpKeyExtractor {
    type Key = IpAddr;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, RateLimitError> {
        maybe_connect_info(req).ok_or(RateLimitError::ExtractionError)
    }
}

struct SmartIpKeyExtractor;

impl KeyExtractor for SmartIpKeyExtractor {
    type Key = IpAddr;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, RateLimitError> {
        let headers = req.headers();
        maybe_x_forwarded_for(headers)
            .or_else(|| maybe_x_real_ip(headers))
            .or_else(|| maybe_forwarded(headers))
            .or_else(|| maybe_connect_info(req))
            .ok_or(RateLimitError::ExtractionError)
    }
}

// Following chunk yoinked from tower_governor crate.
// See https://github.com/benwis/tower-governor/blob/main/src/key_extractor.rs

const X_REAL_IP: &str = "x-real-ip";
const X_FORWARDED_FOR: &str = "x-forwarded-for";

/// Tries to parse the `x-forwarded-for` header
fn maybe_x_forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get(X_FORWARDED_FOR)
        .and_then(|hv| hv.to_str().ok())
        .and_then(|hstr| {
            hstr.split(',')
                .find_map(|sp| sp.trim().parse::<IpAddr>().ok())
        })
}

/// Tries to parse the `x-real-ip` header
fn maybe_x_real_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get(X_REAL_IP)
        .and_then(|hv| hv.to_str().ok())
        .and_then(|hstr| hstr.parse::<IpAddr>().ok())
}

/// Tries to parse `forwarded` headers
fn maybe_forwarded(headers: &HeaderMap) -> Option<IpAddr> {
    headers.get_all(FORWARDED).iter().find_map(|hv| {
        hv.to_str()
            .ok()
            .and_then(|hstr| ForwardedHeaderValue::from_forwarded(hstr).ok())
            .and_then(|fhv| {
                fhv.iter()
                    .filter_map(|fs| fs.forwarded_for.as_ref())
                    .find_map(|ff| match ff {
                        Identifier::SocketAddr(addr) => Some(addr.ip()),
                        Identifier::IpAddr(ip) => Some(*ip),
                        _ => None,
                    })
            })
    })
}

/// Looks in `ConnectInfo` extension
fn maybe_connect_info<T>(req: &Request<T>) -> Option<IpAddr> {
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip())
}
