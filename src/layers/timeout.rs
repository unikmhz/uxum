use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Handler request timeout configuration
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct HandlerTimeoutsConfig {
    /// Allow passing client-supplied ISO8601 timeout duration in an X-Timeout HTTP header
    #[serde(default = "crate::util::default_true")]
    pub use_x_timeout: bool,
    /// Default timeout for a handler
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
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
    /// Predicate to skip serializing timeout for [`serde`]
    pub fn is_default(this: &Option<Self>) -> bool {
        match this {
            None => true,
            Some(cfg) if *cfg == Self::default() => true,
            _ => false,
        }
    }
}
