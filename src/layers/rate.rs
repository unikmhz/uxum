use std::time::Duration;

use serde::{Deserialize, Serialize};

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HandlerRateLimitConfig {
    ///
    #[serde(default)]
    key: RateLimitKey,
    ///
    burst: u32,
    ///
    #[serde(
        default = "HandlerRateLimitConfig::default_period",
        with = "humantime_serde"
    )]
    period: Duration,
    ///
    #[serde(default)]
    extra_headers: bool,
}

impl HandlerRateLimitConfig {
    fn default_period() -> Duration {
        Duration::from_millis(1)
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
