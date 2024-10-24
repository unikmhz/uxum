//! Application configuration structures.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    apidoc::ApiDocBuilder,
    auth::AuthConfig,
    http_client::HttpClientConfig,
    layers::{
        buffer::HandlerBufferConfig, cors::CorsConfig, rate::HandlerRateLimitConfig,
        timeout::HandlerTimeoutConfig,
    },
    logging::LoggingConfig,
    metrics::MetricsBuilder,
    probes::ProbeConfig,
    runtime::RuntimeConfig,
    telemetry::OpenTelemetryConfig,
    tracing::TracingConfig,
};

/// Top-level application configuration.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct AppConfig {
    /// Tokio runtime configuration.
    #[serde(default)]
    pub runtime: RuntimeConfig,
    /// Logging configuration.
    #[serde(default)]
    pub logging: LoggingConfig,
    /// Tracing configuration.
    #[serde(default)]
    pub tracing: Option<TracingConfig>,
    /// Individual handler configuration.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub handlers: HashMap<String, HandlerConfig>,
    /// API doc configuration.
    #[serde(default)]
    pub api_doc: Option<ApiDocBuilder>,
    /// Metrics configuration.
    #[serde(default)]
    pub metrics: MetricsBuilder,
    /// Probes and maintenance mode configuration.
    #[serde(default)]
    pub probes: ProbeConfig,
    /// Common OpenTelemetry configuration.
    #[serde(default)]
    pub otel: OpenTelemetryConfig,
    /// Authentication and authorization back-end configuration.
    #[serde(default)]
    pub auth: AuthConfig,
    /// [`reqwest`] HTTP client configuration.
    #[serde(default)]
    pub http_clients: HashMap<String, HttpClientConfig>,
    /// Short application name.
    #[serde(skip)]
    pub app_name: Option<String>,
    /// Application version.
    #[serde(skip)]
    pub app_version: Option<String>,
    /// OpenTelemetry static attributes.
    #[serde(skip)]
    pub otel_res: Option<opentelemetry_sdk::Resource>,
}

impl AppConfig {
    /// Set short name of an application.
    ///
    /// Whitespace is not allowed, as this value is used in Server: HTTP header, among other
    /// things.
    pub fn with_app_name(&mut self, app_name: impl ToString) -> &mut Self {
        // TODO: maybe check for value correctness?
        self.app_name = Some(app_name.to_string());
        self
    }

    /// Set application version.
    ///
    /// Preferably in semver format. Whitespace is not allowed, as this value is used in Server:
    /// HTTP header, among other things.
    pub fn with_app_version(&mut self, app_version: impl ToString) -> &mut Self {
        // TODO: maybe check for value correctness?
        self.app_version = Some(app_version.to_string());
        self
    }
}

/// Configuration of a single handler.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
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
    /// CORS configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cors: Option<CorsConfig>,
    /// Rate limiter configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<HandlerRateLimitConfig>,
    /// Throttling configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throttle: Option<u8>,
    /// Request timeout configuration.
    #[serde(default, skip_serializing_if = "HandlerTimeoutConfig::is_default")]
    pub timeout: HandlerTimeoutConfig,
    /// Required RBAC permissions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<String>,
}
