use std::{
    collections::HashMap,
    time::Duration,
};

use serde::{Deserialize, Serialize};

///
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct HandlersConfig {
    ///
    pub handlers: HashMap<String, HandlerConfig>,
}

/// Configuration of a single handler.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct HandlerConfig {
    /// Method is completely disabled at runtime.
    #[serde(default)]
    disabled: bool,
    /// Method is hidden from OpenAPI specification.
    #[serde(default)]
    hidden: bool,
    /// Circuit breaker configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cb: Option<HandlerCircuitBreakerConfig>,
    /// Rate limiter configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rate_limit: Option<usize>,
    /// Throttling configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    throttle: Option<u8>,
    /// Request timeout configuration.
    #[serde(default, skip_serializing_if = "HandlerTimeoutsConfig::is_default")]
    timeout: Option<HandlerTimeoutsConfig>,
    /// Required RBAC roles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    roles: Vec<String>,
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HandlerCircuitBreakerConfig {
    ///
    #[serde(default, skip_serializing_if = "Option::is_none")]
    consecutive_failures: Option<ConsecutiveFailuresConfig>,
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ConsecutiveFailuresConfig {
    ///
    num_failures: i32,
}

///
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct HandlerTimeoutsConfig {
    /// Allow passing client-supplied ISO8601 timeout duration in an X-Timeout HTTP header.
    #[serde(default = "HandlerTimeoutsConfig::default_allow_x_timeout")]
    use_x_timeout: bool,
    /// Default timeout for a handler.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "humantime_serde")]
    default_timeout: Option<Duration>,
    /// Minimum allowed timeout for a method.
    ///
    /// Timeout durations less than this value will automatically be responded
    /// with a 504 HTTP status code.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "humantime_serde")]
    min_timeout: Option<Duration>,
    /// Maximum allowed timeout for a method.
    ///
    /// Timeout durations over this value will automatically be responded
    /// with a 504 HTTP status code.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "humantime_serde")]
    max_timeout: Option<Duration>,
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

    ///
    fn default_allow_x_timeout() -> bool {
        true
    }
}
