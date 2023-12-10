use serde::{Deserialize, Serialize};

/// Circuit breaker middleware configuration
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HandlerCircuitBreakerConfig {
    ///
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consecutive_failures: Option<ConsecutiveFailuresConfig>,
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ConsecutiveFailuresConfig {
    ///
    pub num_failures: i32,
}
