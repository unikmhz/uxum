//! Rate limiting [`tower`] layer.
//!
//! # Quota
//!
//! Limits are enforced with GCRA from [`governor`], which behaves like a token bucket that
//! refills continuously. There are no fixed or sliding windows, and nothing resets on second
//! boundaries.
//!
//! - `rps` is the refill rate: one request becomes available every `1/rps` seconds.
//! - Bucket capacity is `max(burst_rps, rps) * burst_duration` requests, rounded up. Buckets
//!   start full, so an idle client can send that many requests at once. A client sending at
//!   `burst_rps` lasts longer than `burst_duration`, because the bucket keeps refilling at `rps`.
//! - With defaults (`burst_duration` of 1s, no `burst_rps`) capacity equals `rps`.
//! - Rejected requests take nothing from the bucket, so retrying while limited does not delay
//!   recovery.
//!
//! For example, `rps: 10, burst_rps: 50, burst_duration: 2s` gives a bucket of 100: an idle
//! client can send 100 requests instantly, or 50 per second for 2.5 seconds, and then 10 per
//! second. An empty bucket refills completely in 10 seconds.
//!
//! # Keys
//!
//! `global` is a single bucket shared by all callers of a handler. Other keys give every IP
//! address or user a bucket of their own.
//!
//! - `smart_ip` takes the leftmost address from `X-Forwarded-For`, `X-Real-IP` or `Forwarded`
//!   headers. Clients can forge them, so use it only behind a proxy that replaces these headers
//!   instead of appending to them.
//! - `user_id` relies on the authentication layer, which runs before rate limiting.
//! - `peer_ip` needs the app to be served with `into_make_service_with_connect_info`.
//!
//! Requests whose key cannot be determined (anonymous requests under `user_id`, for example) are
//! not limited by that limit. Rejecting them is up to authentication or another limit type (i.e.
//! IP-based limit). This lets per-user and per-IP limits work together: anonymous requests are
//! only limited per IP, registered users are limited per user. A limit with `require_key` rejects
//! such requests with HTTP 400 instead.
//!
//! State lives in memory, separately for each handler and process. Limits are not shared between
//! replicas, so with N replicas a client may get up to N times the configured rate. Restarts reset
//! all buckets. Keys whose bucket has refilled are forgotten periodically, which is lossless: a
//! full bucket is indistinguishable from a new one.
//!
//! # Multiple limits and overrides
//!
//! A request has to pass every limit configured for a handler, in order. Capacity taken by a limit
//! is not returned when a later one rejects the request, so narrow limits (per user) go before wide
//! ones (global): otherwise a client over its own limit keeps draining the shared budget.
//!
//! An override gives a single key its own bucket. Fields it does not set are inherited from the
//! limit, `burst_rps` included, so lowering `rps` for a key under a limit with a large `burst_rps`
//! keeps that large burst.
//!
//! A limit without `rps` applies only to keys with an override, and everyone else passes.

use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    future::Future,
    marker::PhantomData,
    num::NonZeroU32,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    task::{Context, Poll, ready},
    time::Duration,
};

use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    response::IntoResponse,
};
use dashmap::DashMap;
use governor::{
    Quota, RateLimiter,
    clock::{Clock, DefaultClock, QuantaClock, QuantaInstant},
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
};
use pin_project::pin_project;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{
        self, MapAccess, SeqAccess, Visitor,
        value::{MapAccessDeserializer, SeqAccessDeserializer},
    },
};
use thiserror::Error;
use tower::{BoxError, Layer, Service};
use tracing::{trace_span, warn};

use crate::{
    errors,
    layers::util::{
        ExtractionError, KeyExtractor, PeerIpKeyExtractor, SmartIpKeyExtractor, UserIdKeyExtractor,
    },
};

/// Error type returned by rate-limiting layer.
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum RateLimitError {
    /// Extraction error.
    #[error(transparent)]
    Extraction(#[from] ExtractionError),
    /// Rate limit exceeded.
    #[error("Rate limit reached: available after {remaining_seconds} seconds")]
    LimitReached {
        /// Remaining seconds until method becomes available again.
        ///
        /// NOTE: Retry-After cannot be specified with fractional digits as per RFC 9110.
        remaining_seconds: u64,
    },
}

impl RateLimitError {
    /// HTTP status code for used for this error.
    fn http_status(&self) -> StatusCode {
        match self {
            Self::LimitReached { .. } => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoResponse for RateLimitError {
    fn into_response(self) -> Response<Body> {
        let mut resp = problemdetails::new(self.http_status())
            .with_type(errors::TAG_UXUM_RATE_LIMIT)
            .with_title(self.to_string());
        if let Self::LimitReached { remaining_seconds } = self {
            resp = resp.with_value("retry_after", remaining_seconds);
        }
        resp.into_response()
    }
}

/// Configuration for rate-limiting layer.
///
/// Uses [`governor`] crate internally.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
// A misspelled field name would otherwise quietly turn the limit off.
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct HandlerRateLimitConfig {
    /// Key extractor used to find rate-limiting bucket.
    #[serde(default)]
    key: RateLimitKey,
    /// Sustained requests per second.
    ///
    /// When not set, only keys listed in [`Self::overrides`] are limited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rps: Option<NonZeroU32>,
    /// Maximum requests per second during burst.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    burst_rps: Option<NonZeroU32>,
    /// Duration of burst, used for bucket size calculation.
    #[serde(
        default = "HandlerRateLimitConfig::default_burst_duration",
        with = "humantime_serde"
    )]
    burst_duration: Duration,
    /// Quotas for individual keys, replacing the defaults above.
    ///
    /// Map keys correspond to the extracted key format (user ID, IP address,
    /// etc.). Ignored for [`RateLimitKey::Global`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    overrides: BTreeMap<String, QuotaOverride>,
    /// Reject requests whose key cannot be determined with HTTP 400, instead of letting them
    /// through. Has no effect for [`RateLimitKey::Global`].
    #[serde(default)]
    require_key: bool,
}

/// Quota replacing the default one for a single rate-limiting key.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct QuotaOverride {
    /// Sustained requests per second.
    rps: Option<NonZeroU32>,
    /// Maximum requests per second during burst.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    burst_rps: Option<NonZeroU32>,
    /// Duration of burst. Inherited from the enclosing limiter when not specified.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    burst_duration: Option<Duration>,
}

impl HandlerRateLimitConfig {
    /// Default value for [`Self::burst_duration`].
    #[must_use]
    #[inline]
    fn default_burst_duration() -> Duration {
        Duration::from_secs(1)
    }

    /// Helper method to calculate governor burst size. Returns `None` when `rps` is not set.
    pub fn burst_size(&self) -> Option<NonZeroU32> {
        let rps = self.rps?;
        let burst_rps = match self.burst_rps {
            Some(burst_rps) if burst_rps > rps => burst_rps,
            _ => rps,
        };
        let seconds = self.burst_duration.as_secs_f64();
        Some(NonZeroU32::new((seconds * f64::from(burst_rps.get())).ceil() as u32).unwrap_or(rps))
    }

    /// Helper method for governor period calculation. Returns `None` when `rps` is not set.
    pub fn period(&self) -> Option<Duration> {
        self.rps.map(|rps| Duration::from_secs(1) / rps.get())
    }

    /// Governor quota, or `None` when [`Self::rps`] is not set.
    fn quota(&self) -> Option<Quota> {
        Some(
            Quota::with_period(self.period()?)
                .unwrap()
                .allow_burst(self.burst_size()?),
        )
    }

    /// Create layer for use in [`tower`] services.
    pub fn make_layer<S, T>(&self) -> RateLimitLayer<S, T> {
        std::slice::from_ref(self).into()
    }

    /// Copy of this config with quota replaced by an override.
    fn with_override(&self, quota: &QuotaOverride) -> Self {
        Self {
            rps: quota.rps.or(self.rps),
            burst_rps: quota.burst_rps.or(self.burst_rps),
            burst_duration: quota.burst_duration.unwrap_or(self.burst_duration),
            overrides: BTreeMap::new(),
            ..self.clone()
        }
    }
}

/// Deserialize either a single rate limit, a list of them, or nothing.
///
/// A visitor is used instead of an untagged enum, which would replace every field error with
/// a generic "did not match any variant" message.
pub(crate) fn deserialize_rate_limits<'de, D>(
    deserializer: D,
) -> Result<Vec<HandlerRateLimitConfig>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OneOrMany;

    impl<'de> Visitor<'de> for OneOrMany {
        type Value = Vec<HandlerRateLimitConfig>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a rate limit or a list of rate limits")
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
            HandlerRateLimitConfig::deserialize(MapAccessDeserializer::new(map))
                .map(|limit| vec![limit])
        }

        fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
            Vec::deserialize(SeqAccessDeserializer::new(seq))
        }
    }

    deserializer.deserialize_any(OneOrMany)
}

/// Method of key extraction for rate limiting.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
enum RateLimitKey {
    /// Global rate limit.
    #[default]
    Global,
    /// Per-peer-IP-address rate limit.
    PeerIp,
    /// Smart per-peer-IP-address rate limit.
    ///
    /// Same as [`RateLimitKey::PeerIp`], but accounts for addresses passed via
    /// `X-Forwarded-For` and similar headers.
    SmartIp,
    /// Per-authenticated-user-ID rate limit.
    UserId,
}

/// Rate-limiting [`tower`] layer.
pub struct RateLimitLayer<S, T> {
    /// Rate limiter configurations. A request has to pass all rate limits to succeed.
    configs: Vec<HandlerRateLimitConfig>,
    /// Inner service type.
    _phantom_service: PhantomData<S>,
    /// Request body type.
    _phantom_request: PhantomData<T>,
}

impl<S, T> From<&[HandlerRateLimitConfig]> for RateLimitLayer<S, T> {
    fn from(value: &[HandlerRateLimitConfig]) -> Self {
        // TODO: don't clone, but share, for runtime updates maybe?
        Self {
            configs: value.to_vec(),
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
        RateLimit::new(service, &self.configs)
    }
}

/// Rate-limiting [`tower`] service.
pub struct RateLimit<S, T> {
    /// Inner service.
    inner: S,
    /// Rate limiters. A request has to pass all of them.
    limiters: Arc<[Box<dyn Limiter<T> + Send + Sync>]>,
}

impl<S, T> Clone for RateLimit<S, T>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            limiters: Arc::clone(&self.limiters),
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
            self.limiters
                .iter()
                .try_for_each(|limiter| limiter.check_limit(&req))
        };
        match rate_result {
            Ok(()) => RateLimitFuture::Positive {
                inner: self.inner.call(req),
            },
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
    /// Create new rate limiting service.
    #[must_use]
    pub fn new(inner: S, configs: &[HandlerRateLimitConfig]) -> Self {
        Self {
            inner,
            limiters: configs.iter().filter_map(Self::make_limiter).collect(),
        }
    }

    /// Create a limiter matching the configured key extraction method.
    ///
    /// Returns `None` when the limiter would never reject a request.
    fn make_limiter(config: &HandlerRateLimitConfig) -> Option<Box<dyn Limiter<T> + Send + Sync>> {
        let limiter: Box<dyn Limiter<T> + Send + Sync> = match config.key {
            RateLimitKey::Global => {
                if !config.overrides.is_empty() {
                    warn!("ignoring rate limit overrides for global limiter");
                }
                Box::new(GlobalLimiter::new(config.quota()?))
            }
            RateLimitKey::PeerIp => Box::new(KeyedLimiter::new(PeerIpKeyExtractor, config)?),
            RateLimitKey::SmartIp => Box::new(KeyedLimiter::new(SmartIpKeyExtractor, config)?),
            RateLimitKey::UserId => Box::new(KeyedLimiter::new(UserIdKeyExtractor, config)?),
        };
        Some(limiter)
    }
}

/// Rate-limiting [`tower`] service future.
#[pin_project(project = ProjectedOutcome)]
pub enum RateLimitFuture<F> {
    /// Happy path, calling inner service.
    Positive {
        /// Inner future.
        #[pin]
        inner: F,
    },
    /// Key extraction error or rate limit exceeded.
    Negative {
        /// Cause of negative response.
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

/// Trait for all rate limiters.
trait Limiter<T> {
    /// Check whether a request can pass through a rate-limiter.
    fn check_limit(&self, req: &Request<T>) -> Result<(), RateLimitError>;
}

/// Global rate limiter.
///
/// Does no key extraction from requests.
struct GlobalLimiter {
    /// Internal limiter state.
    limiter: RateLimiter<NotKeyed, InMemoryState, QuantaClock, NoOpMiddleware<QuantaInstant>>,
}

impl<T> Limiter<T> for GlobalLimiter {
    fn check_limit(&self, _req: &Request<T>) -> Result<(), RateLimitError> {
        self.check()
    }
}

impl GlobalLimiter {
    /// Check whether a request can pass through this limiter.
    fn check(&self) -> Result<(), RateLimitError> {
        self.limiter.check().map_err(|neg| {
            let remaining_seconds = neg.wait_time_from(DefaultClock::default().now()).as_secs();
            RateLimitError::LimitReached { remaining_seconds }
        })
    }

    /// Create new global limiter.
    #[must_use]
    fn new(quota: Quota) -> Self {
        Self {
            limiter: RateLimiter::direct(quota),
        }
    }
}

/// Keyed rate limiter
///
/// Extracts key data from requests using provided [`Self::extractor`].
struct KeyedLimiter<K: KeyExtractor> {
    /// Key extractor.
    extractor: K,
    /// Internal keyed limiter states. Absent when there is no default quota.
    limiters: Option<KeyedState<K::Key>>,
    /// Dedicated limiters for keys with an overridden quota.
    overrides: HashMap<K::Key, GlobalLimiter>,
    /// Number of checks made, used to schedule removal of stale keys.
    checks: AtomicU32,
    /// Reject requests whose key cannot be determined.
    require_key: bool,
}

/// Per-key limiter states sharing a single quota.
type KeyedState<K> =
    RateLimiter<K, DashMap<K, InMemoryState>, QuantaClock, NoOpMiddleware<QuantaInstant>>;

/// Number of keyed checks between removals of stale keys from limiter state.
const EVICTION_INTERVAL: u32 = 4096;

impl<T, K: KeyExtractor> Limiter<T> for KeyedLimiter<K> {
    fn check_limit(&self, req: &Request<T>) -> Result<(), RateLimitError> {
        let key = match self.extractor.extract(req) {
            Ok(key) => key,
            Err(err) if self.require_key => return Err(err.into()),
            // Rejecting requests without a key is the job of authentication, not rate limiting.
            Err(_) => return Ok(()),
        };
        if let Some(limiter) = self.overrides.get(&key) {
            return limiter.check();
        }
        let Some(limiters) = &self.limiters else {
            return Ok(());
        };
        // Keep state from growing forever: forget keys whose quota has fully replenished.
        let check = self.checks.fetch_add(1, Ordering::Relaxed);
        if check.is_multiple_of(EVICTION_INTERVAL) {
            limiters.retain_recent();
            limiters.shrink_to_fit();
        }
        limiters.check_key(&key).map_err(|neg| {
            let remaining_seconds = neg.wait_time_from(DefaultClock::default().now()).as_secs();
            RateLimitError::LimitReached { remaining_seconds }
        })
    }
}

impl<K: KeyExtractor> KeyedLimiter<K> {
    /// Create new keyed limiter.
    ///
    /// Returns `None` when the limiter would never reject a request.
    #[must_use]
    fn new(extractor: K, config: &HandlerRateLimitConfig) -> Option<Self> {
        let limiters = config.quota().map(RateLimiter::keyed);
        let overrides: HashMap<_, _> = config
            .overrides
            .iter()
            .filter_map(|(key, quota)| match key.parse::<K::Key>() {
                // Overrides without rps under a limit without rps impose nothing.
                Ok(key) => Some((
                    key,
                    GlobalLimiter::new(config.with_override(quota).quota()?),
                )),
                Err(_) => {
                    warn!(key, "ignoring rate limit override with invalid key");
                    None
                }
            })
            .collect();
        if limiters.is_none() && overrides.is_empty() && !config.require_key {
            return None;
        }
        Some(Self {
            extractor,
            limiters,
            overrides,
            checks: AtomicU32::new(0),
            require_key: config.require_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, time::Duration};

    use futures::executor::block_on;
    use serde_json::{from_value, json};
    use tower::service_fn;

    use super::*;
    use crate::{auth::UserId, config::HandlerConfig};

    fn limit(value: serde_json::Value) -> HandlerRateLimitConfig {
        from_value(value).unwrap()
    }

    fn request(user: &str) -> Request<()> {
        let mut req = Request::new(());
        req.extensions_mut().insert(UserId::from(user));
        req
    }

    /// Make a number of requests as `user`, returning how many were let through.
    fn allowed<S>(service: &mut S, user: &str, count: usize) -> usize
    where
        S: Service<Request<()>>,
    {
        (0..count)
            .filter(|_| block_on(service.call(request(user))).is_ok())
            .count()
    }

    fn rate_limited(
        configs: &[HandlerRateLimitConfig],
    ) -> impl Service<Request<()>, Response = Response<()>, Error = BoxError> + use<> {
        RateLimit::new(
            service_fn(|_req: Request<()>| async { Ok::<_, Infallible>(Response::new(())) }),
            configs,
        )
    }

    /// Deserialize - single limit or a list of limits.
    #[test]
    fn de_one_or_many() {
        #[derive(Deserialize)]
        struct Wrapper {
            #[serde(deserialize_with = "deserialize_rate_limits")]
            rate_limit: Vec<HandlerRateLimitConfig>,
        }

        let one: Wrapper = from_value(json!({"rate_limit": {"rps": 1}})).unwrap();
        let many: Wrapper =
            from_value(json!({"rate_limit": [{"rps": 1}, {"key": "user_id", "rps": 2}]})).unwrap();
        assert_eq!(one.rate_limit.len(), 1);
        assert_eq!(many.rate_limit.len(), 2);
        assert_eq!(many.rate_limit[1].key, RateLimitKey::UserId);
    }

    /// Parse handler config from YAML the same way applications load it.
    fn handler_config(yaml: &str) -> Result<HandlerConfig, config::ConfigError> {
        config::Config::builder()
            .add_source(config::File::from_str(yaml, config::FileFormat::Yaml))
            .build()?
            .try_deserialize()
    }

    /// Empty limit list is valid, and config mistakes are reported precisely.
    #[test]
    fn de_empty_and_errors() {
        assert!(handler_config("rate_limit:").unwrap().rate_limit.is_empty());
        for yaml in [
            "rate_limit: {rsp: 5}",
            "rate_limit: {overrides: {bob: {rsp: 1}}}",
        ] {
            let err = handler_config(yaml).unwrap_err().to_string();
            assert!(err.contains("unknown field `rsp`"), "{yaml}: {err}");
        }
        let err = handler_config("rate_limit: [{rps: 0}]")
            .unwrap_err()
            .to_string();
        assert!(err.contains("nonzero"), "{err}");
    }

    /// A request has to pass every configured limit.
    #[test]
    fn all_limits_must_pass() {
        let mut service = rate_limited(&[
            limit(json!({"key": "user_id", "rps": 1, "burst_duration": "2s"})),
            limit(json!({"key": "global", "rps": 1, "burst_duration": "3s"})),
        ]);
        // User limit (2) is hit first, and rejected requests don't count towards global one.
        assert_eq!(allowed(&mut service, "alice", 5), 2);
        // Global limit (3) leaves a single request for another user.
        assert_eq!(allowed(&mut service, "bob", 5), 1);
    }

    /// Overridden keys use their own quota, everyone else uses the default one.
    #[test]
    fn overrides_replace_quota() {
        let mut service = rate_limited(&[limit(json!({
            "key": "user_id",
            "rps": 1,
            "burst_duration": "2s",
            "overrides": {
                "alice": {"rps": 1, "burst_duration": "5s"},
                "carol": {"rps": 1, "burst_rps": 4}
            }
        }))]);
        assert_eq!(allowed(&mut service, "alice", 10), 5);
        assert_eq!(allowed(&mut service, "bob", 10), 2);
        // Burst duration is inherited.
        assert_eq!(allowed(&mut service, "carol", 10), 8);
    }

    /// Overrides with unparseable keys are ignored.
    #[test]
    fn invalid_override_key_ignored() {
        let mut service = rate_limited(&[limit(json!({
            "key": "peer_ip",
            "rps": 1,
            "overrides": {"not-an-ip": {"rps": 10}}
        }))]);
        let mut req = Request::new(());
        req.extensions_mut().insert(axum::extract::ConnectInfo(
            "127.0.0.1:1234".parse::<std::net::SocketAddr>().unwrap(),
        ));
        assert!(block_on(service.call(req)).is_ok());
    }

    /// IPv4 override keys match clients reported as IPv4-mapped IPv6 by dual-stack listeners.
    #[test]
    fn ipv4_override_matches_mapped_address() {
        let mut service = rate_limited(&[limit(json!({
            "key": "peer_ip",
            "overrides": {"10.0.0.5": {"rps": 1}}
        }))]);
        let allowed = (0..5)
            .filter(|_| {
                let mut req = Request::new(());
                req.extensions_mut().insert(axum::extract::ConnectInfo(
                    "[::ffff:10.0.0.5]:1234"
                        .parse::<std::net::SocketAddr>()
                        .unwrap(),
                ));
                block_on(service.call(req)).is_ok()
            })
            .count();
        assert_eq!(allowed, 1);
    }

    /// Keys with fully replenished quota are dropped from limiter state.
    #[test]
    fn stale_keys_are_evicted() {
        let limiter = KeyedLimiter::new(
            UserIdKeyExtractor,
            &limit(json!({"key": "user_id", "rps": 1_000_000})),
        )
        .unwrap();
        let state = limiter.limiters.as_ref().unwrap();
        for i in 0..EVICTION_INTERVAL {
            limiter.check_limit(&request(&i.to_string())).unwrap();
        }
        assert_eq!(state.len(), EVICTION_INTERVAL as usize);
        std::thread::sleep(Duration::from_millis(50));
        limiter.check_limit(&request("last")).unwrap();
        assert!(state.len() < 2);
    }

    /// Without default rps, only overridden keys are limited.
    #[test]
    fn no_default_rps_limits_only_overrides() {
        let mut service = rate_limited(&[limit(json!({
            "key": "user_id",
            "burst_duration": "2s",
            "overrides": {
                "alice": {"rps": 1},
                "carol": {"burst_rps": 4}
            }
        }))]);
        assert_eq!(allowed(&mut service, "alice", 10), 2);
        assert_eq!(allowed(&mut service, "bob", 10), 10);
        // Neither the override nor the limit sets rps, so there is nothing to enforce.
        assert_eq!(allowed(&mut service, "carol", 10), 10);
    }

    /// Global limit without rps lets everything through.
    #[test]
    fn global_without_rps_is_unlimited() {
        let mut service = rate_limited(&[limit(json!({"key": "global"}))]);
        assert_eq!(allowed(&mut service, "alice", 10), 10);
    }

    /// Anonymous requests skip the per-user limit and are only limited per IP.
    #[test]
    fn anonymous_requests_limited_per_ip_only() {
        let mut service = rate_limited(&[
            limit(json!({"key": "user_id", "rps": 1})),
            limit(json!({"key": "peer_ip", "rps": 1, "burst_duration": "3s"})),
        ]);
        let allowed = (0..5)
            .filter(|_| {
                let mut req = Request::new(());
                req.extensions_mut().insert(axum::extract::ConnectInfo(
                    "127.0.0.1:1234".parse::<std::net::SocketAddr>().unwrap(),
                ));
                block_on(service.call(req)).is_ok()
            })
            .count();
        assert_eq!(allowed, 3);
    }

    /// With `require_key`, requests without a key are rejected even when nothing is rate-limited.
    #[test]
    fn require_key_rejects_anonymous() {
        let mut service = rate_limited(&[limit(json!({"key": "user_id", "require_key": true}))]);
        let err = block_on(service.call(Request::new(()))).unwrap_err();
        assert!(matches!(
            err.downcast_ref::<RateLimitError>(),
            Some(RateLimitError::Extraction(_))
        ));
        assert_eq!(allowed(&mut service, "alice", 5), 5);
    }
}
