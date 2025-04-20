//! Application configuration structures.

use std::{collections::HashMap, marker::PhantomData};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    apidoc::ApiDocBuilder,
    auth::AuthConfig,
    builder::server::ServerBuilder,
    http_client::HttpClientConfig,
    layers::{
        buffer::HandlerBufferConfig, cors::CorsConfig, rate::HandlerRateLimitConfig,
        timeout::HandlerTimeoutConfig,
    },
    logging::LoggingConfig,
    metrics::MetricsBuilder,
    probes::ProbeConfig,
    runtime::RuntimeConfig,
    tracing::TracingConfig,
};

/// Root container for app configuration.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct ServiceConfig<C = ()>
where
    C: Clone + std::fmt::Debug + PartialEq,
{
    /// Application configuration.
    #[serde(flatten)]
    pub app: AppConfig,
    /// Server configuration.
    #[serde(default)]
    pub server: ServerBuilder,
    /// Service-specific configuration.
    #[serde(flatten)]
    pub service: C,
}

impl<C> ServiceConfig<C>
where
    C: Clone + std::fmt::Debug + PartialEq,
{
    /// Create builder for service configuration.
    pub fn builder() -> ServiceConfigBuilder<C> {
        ServiceConfigBuilder::new()
    }
}

/// Top-level service configuration error type.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ServiceConfigError {
    /// Configuration builder error
    #[error(transparent)]
    Config(#[from] config::ConfigError),
}

/// Builder for service configuration.
#[must_use]
pub struct ServiceConfigBuilder<C>
where
    C: Clone + std::fmt::Debug + PartialEq,
{
    builder: config::ConfigBuilder<config::builder::DefaultState>,
    _type: PhantomData<C>,
}

impl<C> ServiceConfigBuilder<C>
where
    C: Clone + std::fmt::Debug + PartialEq,
{
    /// Alternative method to construct a service configuration builder.
    pub fn new() -> Self {
        Self {
            builder: config::Config::builder(),
            _type: PhantomData,
        }
    }
}

impl<C> Default for ServiceConfigBuilder<C>
where
    C: Clone + std::fmt::Debug + PartialEq,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<C> ServiceConfigBuilder<C>
where
    C: Clone + std::fmt::Debug + PartialEq + for<'de> Deserialize<'de>,
{
    /// Try to build configuration object from preconfigured sources.
    ///
    /// This method will do all the I/O necessary to load the configuration.
    ///
    /// See [`config::builder::ConfigBuilder::build`].
    ///
    /// # Errors
    ///
    /// Returns `Err` if some configuration loading was unsuccessful.
    pub fn build(self) -> Result<ServiceConfig<C>, ServiceConfigError> {
        self.builder.build()?.try_deserialize().map_err(Into::into)
    }

    /// Add a custom object implementing [`Source`] trait as a source of service configuration.
    ///
    /// [`Source`]: config::Source
    pub fn with_source<T>(mut self, source: T) -> Self
    where
        T: config::Source + Send + Sync + 'static,
    {
        self.builder = self.builder.add_source(source);
        self
    }

    /// Add file as a source of service configuration.
    pub fn with_file(self, name: impl AsRef<str>) -> Self {
        self.with_source(config::File::with_name(name.as_ref()))
    }

    /// Add environment variables as a source of service configuration.
    pub fn with_env(self, prefix: impl AsRef<str>) -> Self {
        self.with_source(
            config::Environment::with_prefix(prefix.as_ref())
                .separator("_")
                .prefix_separator("__"),
        )
    }
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracing: Option<TracingConfig>,
    /// Individual handler configuration.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub handlers: HashMap<String, HandlerConfig>,
    /// API doc configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_doc: Option<ApiDocBuilder>,
    /// Metrics configuration.
    #[serde(default)]
    pub metrics: MetricsBuilder,
    /// Probes and maintenance mode configuration.
    #[serde(default)]
    pub probes: ProbeConfig,
    /// Authentication and authorization back-end configuration.
    #[serde(default)]
    pub auth: AuthConfig,
    /// [`reqwest`] HTTP client configuration.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
