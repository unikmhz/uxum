use std::{collections::HashMap, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{layers::*, logging::LoggingConfig};

/// Top-level application configuration.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct AppConfig {
    /// Logging configuration.
    pub logging: LoggingConfig,
    /// Individual handler configuration.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub handlers: HashMap<String, HandlerConfig>,
}

/// Configuration of a single handler.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct HandlerConfig {
    /// Method is completely disabled at runtime.
    #[serde(default)]
    pub disabled: bool,
    /// Method is hidden from OpenAPI specification.
    #[serde(default)]
    pub hidden: bool,
    /// Request buffering configuration.
    #[serde(default)]
    pub buffer: Option<HandlerBufferConfig>,
    /// Circuit breaker configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cb: Option<HandlerCircuitBreakerConfig>,
    /// Rate limiter configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<HandlerRateLimitConfig>,
    /// Throttling configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throttle: Option<u8>,
    /// Request timeout configuration.
    #[serde(default, skip_serializing_if = "HandlerTimeoutsConfig::is_default")]
    pub timeout: Option<HandlerTimeoutsConfig>,
    /// Required RBAC roles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HandlerTimeoutsConfig {
    /// Allow passing client-supplied ISO8601 timeout duration in an X-Timeout HTTP header.
    #[serde(default = "crate::util::default_true")]
    pub use_x_timeout: bool,
    /// Default timeout for a handler.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub default_timeout: Option<Duration>,
    /// Minimum allowed timeout for a method.
    ///
    /// Timeout durations less than this value will automatically be responded
    /// with a 504 HTTP status code.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub min_timeout: Option<Duration>,
    /// Maximum allowed timeout for a method.
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

impl Default for HandlerTimeoutsConfig {
    fn default() -> Self {
        Self {
            use_x_timeout: true,
            default_timeout: None,
            min_timeout: None,
            max_timeout: None,
        }
    }
}

impl HandlerTimeoutsConfig {
    ///
    fn is_default(this: &Option<Self>) -> bool {
        match this {
            None => true,
            Some(cfg) if *cfg == Self::default() => true,
            _ => false,
        }
    }
}
