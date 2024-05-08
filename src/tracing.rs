use std::time::Duration;

use opentelemetry_otlp::{Protocol, TonicExporterBuilder, WithExportConfig};
use opentelemetry_sdk::{
    runtime::Tokio,
    trace::{Config, RandomIdGenerator, Sampler, Tracer},
    Resource,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug_span, Level, Subscriber};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{
    filter::{Filtered, Targets},
    registry::LookupSpan,
    Layer,
};
use url::Url;

use crate::logging::LoggingLevel;

/// Error type used in tracing configuration
#[derive(Debug, Error)]
pub enum TracingError {
    // OTel tracing error
    #[error("OTel tracing error: {0}")]
    OpenTelemetry(#[from] opentelemetry::trace::TraceError),
}

/// OpenTelemetry tracing configuration
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TracingConfig {
    /// Trace collector endpoint URL
    #[serde(default = "TracingConfig::default_endpoint")]
    endpoint: Url,
    /// Protocol to use when exporting data
    #[serde(default, alias = "format")]
    protocol: TracingProtocol,
    /// OTLP collector timeout
    #[serde(default = "TracingConfig::default_timeout")]
    timeout: Duration,
    /// Sampling rule
    #[serde(default)]
    sample: TracingSampler,
    /// Minimum severity level to export
    #[serde(default)]
    level: LoggingLevel,
    /// Limits configuration
    #[serde(default, flatten)]
    limits: TracingSpanLimits,
    /// Optional features configuration
    #[serde(default)]
    include: TracingIncludes,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            endpoint: Self::default_endpoint(),
            protocol: TracingProtocol::default(),
            timeout: Self::default_timeout(),
            sample: TracingSampler::default(),
            level: LoggingLevel::default(),
            limits: TracingSpanLimits::default(),
            include: TracingIncludes::default(),
        }
    }
}

impl TracingConfig {
    /// Default value for [`Self::endpoint`]
    #[must_use]
    #[inline]
    #[allow(clippy::unwrap_used)]
    fn default_endpoint() -> Url {
        // TODO: check correctness using a unit test
        Url::parse("http://localhost:4317").unwrap()
    }

    /// Default value for [`Self::timeout`]
    #[must_use]
    #[inline]
    fn default_timeout() -> Duration {
        Duration::from_secs(opentelemetry_otlp::OTEL_EXPORTER_OTLP_TIMEOUT_DEFAULT)
    }

    /// Build internal protocol exporter
    fn build_exporter(&self) -> TonicExporterBuilder {
        // TODO: allow adding metadata
        opentelemetry_otlp::new_exporter()
            .tonic()
            .with_protocol(self.protocol.into())
            .with_endpoint(self.endpoint.to_string())
            .with_timeout(self.timeout)
    }

    /// Build OpenTelemetry SDK configuration
    fn build_config(&self, resource: Resource) -> Config {
        Config::default()
            .with_sampler::<Sampler>(self.sample.into())
            .with_id_generator(RandomIdGenerator::default())
            .with_max_events_per_span(self.limits.max_events_per_span)
            .with_max_attributes_per_span(self.limits.max_attributes_per_span)
            .with_max_links_per_span(self.limits.max_links_per_span)
            .with_max_attributes_per_event(self.limits.max_attributes_per_event)
            .with_max_attributes_per_link(self.limits.max_attributes_per_link)
            .with_resource(resource)
    }

    /// Build OpenTelemetry tracing pipeline
    pub fn build_pipeline(&self, resource: Resource) -> Result<Tracer, TracingError> {
        let _span = debug_span!("build_tracing_pipeline").entered();
        opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(self.build_exporter())
            .with_trace_config(self.build_config(resource))
            .install_batch(Tokio)
            .map_err(Into::into)
    }

    /// Build OpenTelemetry layer for [`tracing`]
    pub fn build_layer<S>(
        &self,
        tracer: &Tracer,
    ) -> Filtered<OpenTelemetryLayer<S, Tracer>, Targets, S>
    where
        S: Subscriber + for<'span> LookupSpan<'span>,
    {
        let _span = debug_span!("build_tracing_layer").entered();
        // TODO: additional params from config
        tracing_opentelemetry::layer()
            .with_tracer(tracer.clone())
            .with_location(self.include.location)
            .with_error_fields_to_exceptions(self.include.exception_from_error_fields)
            .with_error_events_to_exceptions(self.include.exception_from_error_events)
            .with_tracked_inactivity(self.include.inactivity)
            .with_threads(self.include.thread_info)
            .with_error_events_to_status(self.include.status_from_error_events)
            .with_filter(
                Targets::new()
                // Filter out internal HTTP/2 tracing, otherwise OTel tracing itself
                // produces more sent traces.
                .with_target("h2", Level::WARN)
                .with_default(self.level),
            )
    }
}

/// Configuration of optional data to include in tracing objects
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct TracingIncludes {
    /// Include file/module/line in span and event attributes
    #[serde(default = "crate::util::default_true")]
    location: bool,
    /// Convert [`std::error::Error`] values into `exception.*` fields
    #[serde(default = "crate::util::default_true")]
    exception_from_error_fields: bool,
    /// Convert events with `error` field into `exception.*` fields
    #[serde(default = "crate::util::default_true")]
    exception_from_error_events: bool,
    /// Set status error description from exception events
    #[serde(default = "crate::util::default_true")]
    status_from_error_events: bool,
    /// Track both busy and inactive times for spans
    #[serde(default)]
    inactivity: bool,
    /// Record thread name/ID in span attributes
    #[serde(default = "crate::util::default_true")]
    thread_info: bool,
}

/// Limits on number of properties in various tracing objects
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct TracingSpanLimits {
    /// The max events that can be added to a Span
    #[serde(default = "TracingSpanLimits::default_max")]
    max_events_per_span: u32,
    /// The max attributes that can be added to a Span
    #[serde(default = "TracingSpanLimits::default_max")]
    max_attributes_per_span: u32,
    /// The max links that can be added to a Span
    #[serde(default = "TracingSpanLimits::default_max")]
    max_links_per_span: u32,
    /// The max attributes that can be added into an Event
    #[serde(default = "TracingSpanLimits::default_max")]
    max_attributes_per_event: u32,
    /// The max attributes that can be added into a Link
    #[serde(default = "TracingSpanLimits::default_max")]
    max_attributes_per_link: u32,
}

impl Default for TracingSpanLimits {
    fn default() -> Self {
        Self {
            max_events_per_span: Self::default_max(),
            max_attributes_per_span: Self::default_max(),
            max_links_per_span: Self::default_max(),
            max_attributes_per_event: Self::default_max(),
            max_attributes_per_link: Self::default_max(),
        }
    }
}

impl TracingSpanLimits {
    /// Default value for all attributes
    #[must_use]
    #[inline]
    fn default_max() -> u32 {
        128
    }
}

/// Protocol to use for data export
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
enum TracingProtocol {
    /// GRPC over HTTP
    #[default]
    OtlpGrpc,
    /// Protobuf over HTTP
    OtlpHttp,
}

impl From<TracingProtocol> for Protocol {
    fn from(value: TracingProtocol) -> Self {
        match value {
            TracingProtocol::OtlpGrpc => Self::Grpc,
            TracingProtocol::OtlpHttp => Self::HttpBinary,
        }
    }
}

/// Trace sampling configuration
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TracingSampler {
    /// Always export data
    #[default]
    Always,
    /// Export a specified fraction of all data
    Fraction(f64),
}

impl From<TracingSampler> for Sampler {
    fn from(value: TracingSampler) -> Self {
        match value {
            TracingSampler::Always => Self::AlwaysOn,
            TracingSampler::Fraction(frac) => Self::TraceIdRatioBased(frac),
        }
    }
}
