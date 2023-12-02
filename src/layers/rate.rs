use std::{num::NonZeroU32, time::Duration};

use governor::{clock::QuantaInstant, middleware::RateLimitingMiddleware};
use hyper::{Request, Response};
use serde::{Deserialize, Serialize};
use tower::{BoxError, Layer, Service};
use tower_governor::{
    governor::{Governor, GovernorConfig, GovernorConfigBuilder},
    key_extractor::{GlobalKeyExtractor, KeyExtractor, PeerIpKeyExtractor, SmartIpKeyExtractor},
};

use crate::util::BoxCloneLayer;

/// Configuration for rate-limiting layer.
///
/// Uses [`tower_governor`] crate internally.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HandlerRateLimitConfig {
    /// Key extractor used to find rate-limiting bucket.
    #[serde(default)]
    key: RateLimitKey,
    /// Sustained requests per second.
    rps: NonZeroU32,
    /// Maximum requests per second during burst.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    burst_rps: Option<NonZeroU32>,
    /// Duration of burst, used for bucket size calculation.
    #[serde(
        default = "HandlerRateLimitConfig::default_burst_duration",
        with = "humantime_serde"
    )]
    burst_duration: Duration,
    /// Add additional headers to response.
    ///
    /// See [`GovernorConfigBuilder::use_headers()`].
    #[serde(default)]
    extra_headers: bool,
}

impl HandlerRateLimitConfig {
    fn default_burst_duration() -> Duration {
        Duration::from_secs(1)
    }

    /// Helper method for governor burst_size calculation.
    pub fn burst_size(&self) -> u32 {
        let rps = match self.burst_rps {
            Some(rps) if rps > self.rps => rps,
            _ => self.rps,
        };
        (self.burst_duration.as_secs_f64() * f64::from(rps.get())).ceil() as u32
    }

    /// Helper method for governor period calculation.
    pub fn period(&self) -> Duration {
        Duration::from_secs(1) / self.rps.get()
    }

    /// Create layer for use in tower services.
    pub fn make_layer<S, T, U>(&self) -> Option<BoxCloneLayer<S, Request<T>, S::Response, BoxError>>
    where
        S: Service<Request<T>, Response = Response<U>> + Send + Sync + Clone + 'static,
        S::Future: Send + 'static,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        let mut config = GovernorConfigBuilder::default();
        config.burst_size(self.burst_size()).period(self.period());
        match (self.key, self.extra_headers) {
            (RateLimitKey::Global, true) => config
                .key_extractor(GlobalKeyExtractor)
                .use_headers()
                .finish()
                .map(|c| BoxCloneLayer::new(OwnedGovernorLayer::from(c))),
            (RateLimitKey::Global, false) => config
                .key_extractor(GlobalKeyExtractor)
                .finish()
                .map(|c| BoxCloneLayer::new(OwnedGovernorLayer::from(c))),
            (RateLimitKey::PeerIp, true) => config
                .key_extractor(PeerIpKeyExtractor)
                .use_headers()
                .finish()
                .map(|c| BoxCloneLayer::new(OwnedGovernorLayer::from(c))),
            (RateLimitKey::PeerIp, false) => config
                .key_extractor(PeerIpKeyExtractor)
                .finish()
                .map(|c| BoxCloneLayer::new(OwnedGovernorLayer::from(c))),
            (RateLimitKey::SmartIp, true) => config
                .key_extractor(SmartIpKeyExtractor)
                .use_headers()
                .finish()
                .map(|c| BoxCloneLayer::new(OwnedGovernorLayer::from(c))),
            (RateLimitKey::SmartIp, false) => config
                .key_extractor(SmartIpKeyExtractor)
                .finish()
                .map(|c| BoxCloneLayer::new(OwnedGovernorLayer::from(c))),
        }
    }
}

///
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitKey {
    ///
    #[default]
    Global,
    ///
    PeerIp,
    ///
    SmartIp,
}

pub struct OwnedGovernorLayer<K, M>
where
    K: KeyExtractor,
    M: RateLimitingMiddleware<QuantaInstant>,
{
    config: GovernorConfig<K, M>,
}

impl<K, M, S> Layer<S> for OwnedGovernorLayer<K, M>
where
    K: KeyExtractor,
    M: RateLimitingMiddleware<QuantaInstant>,
    S: Clone,
{
    type Service = Governor<K, M, S>;

    fn layer(&self, inner: S) -> Self::Service {
        Governor::new(inner, &self.config)
    }
}

impl<K, M> From<GovernorConfig<K, M>> for OwnedGovernorLayer<K, M>
where
    K: KeyExtractor,
    M: RateLimitingMiddleware<QuantaInstant>,
{
    fn from(config: GovernorConfig<K, M>) -> Self {
        Self { config }
    }
}
