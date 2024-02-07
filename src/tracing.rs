use std::time::Duration;

use opentelemetry_otlp::{Protocol, TonicExporterBuilder, WithExportConfig};
use opentelemetry_sdk::{
    runtime::Tokio,
    trace::{Config, RandomIdGenerator, Sampler, Tracer},
    Resource,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug_span, Subscriber};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::registry::LookupSpan;
use url::Url;

/// Error type used in tracing configuration
#[derive(Debug, Error)]
pub enum TracingError {
    #[error("OTel tracing error: {0}")]
    OpenTelemetry(#[from] opentelemetry::trace::TraceError),
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TracingConfig {
    ///
    #[serde(default = "TracingConfig::default_endpoint")]
    endpoint: Url,
    ///
    #[serde(default)]
    format: TracingFormat,
    ///
    #[serde(default = "TracingConfig::default_timeout")]
    timeout: Duration,
    ///
    #[serde(default)]
    sample: TracingSampler,
    ///
    #[serde(default, flatten)]
    limits: TracingSpanLimits,
    ///
    #[serde(default)]
    include: TracingIncludes,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            endpoint: Self::default_endpoint(),
            format: TracingFormat::default(),
            timeout: Self::default_timeout(),
            sample: TracingSampler::default(),
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

    fn build_exporter(&self) -> TonicExporterBuilder {
        // TODO: allow adding metadata
        opentelemetry_otlp::new_exporter()
            .tonic()
            .with_protocol(self.format.as_protocol())
            .with_endpoint(self.endpoint.to_string())
            .with_timeout(self.timeout)
    }

    fn build_config(&self, resource: Resource) -> Config {
        Config::default()
            .with_sampler(self.sample.as_sampler())
            .with_id_generator(RandomIdGenerator::default())
            .with_max_events_per_span(self.limits.max_events_per_span)
            .with_max_attributes_per_span(self.limits.max_attributes_per_span)
            .with_max_links_per_span(self.limits.max_links_per_span)
            .with_max_attributes_per_event(self.limits.max_attributes_per_event)
            .with_max_attributes_per_link(self.limits.max_attributes_per_link)
            .with_resource(resource)
    }

    pub fn build_pipeline(&self, resource: Resource) -> Result<Tracer, TracingError> {
        let _span = debug_span!("build_tracing_pipeline").entered();
        opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(self.build_exporter())
            .with_trace_config(self.build_config(resource))
            .install_batch(Tokio)
            .map_err(Into::into)
    }

    pub fn build_layer<S>(&self, tracer: &Tracer) -> OpenTelemetryLayer<S, Tracer>
    where
        S: Subscriber + for<'span> LookupSpan<'span>,
    {
        // TODO: additional params from config
        tracing_opentelemetry::layer()
            .with_tracer(tracer.clone())
            .with_location(self.include.location)
            .with_error_fields_to_exceptions(self.include.exception_from_error_fields)
            .with_error_events_to_exceptions(self.include.exception_from_error_events)
            .with_tracked_inactivity(self.include.inactivity)
            .with_threads(self.include.thread_info)
            .with_error_events_to_status(self.include.status_from_error_events)
    }
}

///
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct TracingIncludes {
    ///
    #[serde(default)]
    location: bool,
    ///
    #[serde(default)]
    exception_from_error_fields: bool,
    ///
    #[serde(default)]
    exception_from_error_events: bool,
    ///
    #[serde(default)]
    status_from_error_events: bool,
    ///
    #[serde(default)]
    inactivity: bool,
    ///
    #[serde(default)]
    thread_info: bool,
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct TracingSpanLimits {
    ///
    #[serde(default = "TracingSpanLimits::default_max")]
    max_events_per_span: u32,
    ///
    #[serde(default = "TracingSpanLimits::default_max")]
    max_attributes_per_span: u32,
    ///
    #[serde(default = "TracingSpanLimits::default_max")]
    max_links_per_span: u32,
    ///
    #[serde(default = "TracingSpanLimits::default_max")]
    max_attributes_per_event: u32,
    ///
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
    ///
    #[must_use]
    #[inline]
    fn default_max() -> u32 {
        128
    }
}

///
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TracingFormat {
    ///
    #[default]
    OtlpGrpc,
    ///
    OtlpHttp,
}

impl TracingFormat {
    ///
    fn as_protocol(self) -> Protocol {
        match self {
            Self::OtlpGrpc => Protocol::Grpc,
            Self::OtlpHttp => Protocol::HttpBinary,
        }
    }
}

///
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TracingSampler {
    ///
    #[default]
    Always,
    ///
    Fraction(f64),
}

impl TracingSampler {
    ///
    fn as_sampler(&self) -> Sampler {
        match self {
            Self::Always => Sampler::AlwaysOn,
            Self::Fraction(frac) => Sampler::TraceIdRatioBased(*frac),
        }
    }
}
