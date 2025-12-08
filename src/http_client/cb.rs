//! HTTP client - circuit breaker.

use std::time::Duration;

use http::Extensions;
use recloser::{AsyncRecloser, Recloser};
use reqwest::{Request, Response};
use reqwest_middleware::{Error, Middleware, Next, Result};
use serde::{Deserialize, Serialize};

/// HTTP client circuit breaker configuration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct HttpClientCircuitBreakerConfig {
    /// Error rate threshold for tripping the breaker.
    ///
    /// Default is 0.5.
    #[serde(
        default = "HttpClientCircuitBreakerConfig::default_error_rate",
        alias = "threshold"
    )]
    pub error_rate: f32,
    /// Size of CB history buffer in closed state. CB will try calculating the error rate after it
    /// has amassed this many responses.
    ///
    /// Default is 100.
    #[serde(
        default = "HttpClientCircuitBreakerConfig::default_closed_len",
        alias = "closed_length"
    )]
    pub closed_len: usize,
    /// Size of CB history buffer in half-open state. CB will try calculating the error rate after
    /// it has amassed this many responses.
    ///
    /// Default is 10.
    #[serde(
        default = "HttpClientCircuitBreakerConfig::default_half_open_len",
        alias = "half_open_length"
    )]
    pub half_open_len: usize,
    /// Time that CB stays open after being tripped. During this time no error rate analysis is
    /// performed. After this period of time elapses CB will transition into half-open state and
    /// will resume error rate analysis.
    ///
    /// Default is 30 seconds.
    #[serde(
        default = "HttpClientCircuitBreakerConfig::default_open_wait",
        alias = "open_timeout",
        alias = "timeout",
        with = "humantime_serde"
    )]
    pub open_wait: Duration,
}

impl Default for HttpClientCircuitBreakerConfig {
    fn default() -> Self {
        Self {
            error_rate: Self::default_error_rate(),
            closed_len: Self::default_closed_len(),
            half_open_len: Self::default_half_open_len(),
            open_wait: Self::default_open_wait(),
        }
    }
}

impl HttpClientCircuitBreakerConfig {
    /// Default value for [`Self::error_rate`].
    #[must_use]
    #[inline]
    fn default_error_rate() -> f32 {
        0.5
    }

    /// Default value for [`Self::closed_len`].
    #[must_use]
    #[inline]
    fn default_closed_len() -> usize {
        100
    }

    /// Default value for [`Self::half_open_len`].
    #[must_use]
    #[inline]
    fn default_half_open_len() -> usize {
        10
    }

    /// Default value for [`Self::open_wait`].
    #[must_use]
    #[inline]
    fn default_open_wait() -> Duration {
        Duration::from_secs(30)
    }

    /// Create [`recloser::AsyncRecloser`] object based on provided configuration.
    #[must_use]
    pub fn make_circuit_breaker(&self) -> AsyncRecloser {
        AsyncRecloser::from(
            Recloser::custom()
                .error_rate(self.error_rate)
                .closed_len(self.closed_len)
                .half_open_len(self.half_open_len)
                .open_wait(self.open_wait)
                .build(),
        )
    }

    /// Create circuit breaker middleware based on provided configuration.
    #[must_use]
    pub fn make_circuit_breaker_middleware(&self) -> CircuitBreakerMiddleware {
        CircuitBreakerMiddleware(self.make_circuit_breaker())
    }
}

/// Circuit breaker middleware.
#[derive(Clone, Debug)]
pub struct CircuitBreakerMiddleware(AsyncRecloser);

impl From<AsyncRecloser> for CircuitBreakerMiddleware {
    fn from(value: AsyncRecloser) -> Self {
        Self(value)
    }
}

/// Circuit breaker rejection error.
#[derive(Clone, Debug, thiserror::Error)]
#[error("Request rejected, circuit breaker is open")]
pub struct CircuitBreakerRejection;

#[async_trait::async_trait]
impl Middleware for CircuitBreakerMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> Result<Response> {
        match self.0.call(next.run(req, extensions)).await {
            Ok(resp) => Ok(resp),
            Err(recloser::Error::Rejected) => Err(Error::middleware(CircuitBreakerRejection)),
            Err(recloser::Error::Inner(err)) => Err(err),
        }
    }
}
