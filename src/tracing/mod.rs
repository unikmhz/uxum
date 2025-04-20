//! Code to set up trace collection, aggregation and transport.

use std::{num::NonZeroUsize, time::Duration};

use opentelemetry_otlp::{ExporterBuildError, Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    trace::{
        BatchConfig, BatchConfigBuilder, BatchSpanProcessor, Sampler, SdkTracerProvider, Tracer
    },
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

/// Error type used in tracing configuration.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TracingError {
    // Exporter builder error.
    #[error("OTel span exporter builder error: {0}")]
    ExporterBuilder(#[from] ExporterBuildError),
    // OTel tracing error.
    #[error("OTel tracing error: {0}")]
    Tracing(#[from] opentelemetry_sdk::trace::TraceError),
}

/// OpenTelemetry tracing configuration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct TracingConfig {
    /// Trace collector endpoint URL.
    #[serde(default = "TracingConfig::default_endpoint")]
    endpoint: Url,
    /// Protocol to use when exporting data.
    #[serde(default, alias = "format")]
    protocol: TracingProtocol,
    /// OTLP collector timeout.
    #[serde(default = "TracingConfig::default_timeout")]
    timeout: Duration,
    /// Sampling rule.
    #[serde(default)]
    sample: TracingSampler,
    /// Minimum severity level to export.
    #[serde(default)]
    level: LoggingLevel,
    /// Limits configuration.
    #[serde(default, flatten)]
    limits: TracingSpanLimits,
    /// Optional features configuration.
    #[serde(default)]
    include: TracingIncludes,
    /// Batch span processor configuration.
    #[serde(default)]
    batch: TracingBatchConfig,
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
            batch: TracingBatchConfig::default(),
        }
    }
}

impl TracingConfig {
    /// Default value for [`Self::endpoint`].
    #[must_use]
    #[inline]
    #[allow(clippy::unwrap_used)]
    fn default_endpoint() -> Url {
        // TODO: check correctness using a unit test.
        Url::parse("http://localhost:4317").unwrap()
    }

    /// Default value for [`Self::timeout`].
    #[must_use]
    #[inline]
    fn default_timeout() -> Duration {
        opentelemetry_otlp::OTEL_EXPORTER_OTLP_TIMEOUT_DEFAULT
    }

    /// Build internal protocol exporter.
    fn build_exporter(&self) -> Result<SpanExporter, ExporterBuildError> {
        // TODO: allow adding metadata.
        SpanExporter::builder()
            .with_tonic()
            .with_protocol(self.protocol.into())
            .with_endpoint(self.endpoint.to_string())
            .with_timeout(self.timeout)
            .build()
    }

    /// Build OpenTelemetry SDK batch configuration.
    fn build_batch_config(&self) -> BatchConfig {
        self.batch.to_builder().build()
    }

    /// Build OpenTelemetry tracing pipeline.
    ///
    /// # Errors
    ///
    /// Returns `Err` if span exporter and/or processor cannot be installed for some reason.
    pub fn build_pipeline(&self, resource: Resource) -> Result<SdkTracerProvider, TracingError> {
        let _span = debug_span!("build_tracing_pipeline").entered();
        let exporter = self.build_exporter()?;
        let processor = BatchSpanProcessor::builder(exporter)
            .with_batch_config(self.build_batch_config())
            .build();
        Ok(opentelemetry_sdk::trace::SdkTracerProvider::builder()
            .with_span_processor(processor)
            .with_resource(resource)
            .with_sampler(Sampler::from(self.sample))
            .with_max_events_per_span(self.limits.max_events_per_span)
            .with_max_attributes_per_span(self.limits.max_attributes_per_span)
            .with_max_links_per_span(self.limits.max_links_per_span)
            .with_max_attributes_per_event(self.limits.max_attributes_per_event)
            .with_max_attributes_per_link(self.limits.max_attributes_per_link)
            .build())
    }

    /// Build OpenTelemetry layer for [`tracing`].
    pub fn build_layer<S>(
        &self,
        tracer: &Tracer,
    ) -> Filtered<OpenTelemetryLayer<S, Tracer>, Targets, S>
    where
        S: Subscriber + for<'span> LookupSpan<'span>,
    {
        let _span = debug_span!("build_tracing_layer").entered();
        // TODO: additional params from config.
        tracing_opentelemetry::layer()
            .with_tracer(tracer.clone())
            .with_location(self.include.location)
            .with_error_fields_to_exceptions(self.include.exception_from_error_fields)
            .with_error_events_to_exceptions(self.include.exception_from_error_events)
            .with_tracked_inactivity(self.include.inactivity)
            .with_threads(self.include.thread_info)
            .with_error_events_to_status(self.include.status_from_error_events)
            .with_filter(
                // Filter out internal HTTP/2 tracing, otherwise OTel tracing itself
                // produces more sent traces.
                Targets::new()
                    .with_target("h2", Level::WARN)
                    .with_default(self.level),
            )
    }
}

/// Protocol to use for data export.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
enum TracingProtocol {
    /// GRPC over HTTP.
    #[default]
    OtlpGrpc,
    /// Protobuf over HTTP.
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

/// Trace sampling configuration.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
enum TracingSampler {
    /// Always export data.
    #[default]
    Always,
    /// Export a specified fraction of all data.
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

/// Limits on number of properties in various tracing objects.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[non_exhaustive]
struct TracingSpanLimits {
    /// Max number of events that can be added to a Span.
    #[serde(default = "TracingSpanLimits::default_max")]
    max_events_per_span: u32,
    /// Max number of attributes that can be added to a Span.
    #[serde(default = "TracingSpanLimits::default_max")]
    max_attributes_per_span: u32,
    /// Max number of links that can be added to a Span.
    #[serde(default = "TracingSpanLimits::default_max")]
    max_links_per_span: u32,
    /// Max number of attributes that can be added into an Event.
    #[serde(default = "TracingSpanLimits::default_max")]
    max_attributes_per_event: u32,
    /// Max number of attributes that can be added into a Link.
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
    /// Default value for all attributes.
    #[must_use]
    #[inline]
    fn default_max() -> u32 {
        128
    }
}

/// Configuration of optional data to include in tracing objects.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[non_exhaustive]
#[allow(clippy::struct_excessive_bools)]
struct TracingIncludes {
    /// Include file/module/line in span and event attributes.
    #[serde(default = "crate::util::default_true")]
    location: bool,
    /// Convert [`std::error::Error`] values into `exception.*` fields.
    #[serde(default = "crate::util::default_true")]
    exception_from_error_fields: bool,
    /// Convert events with `error` field into `exception.*` fields.
    #[serde(default = "crate::util::default_true")]
    exception_from_error_events: bool,
    /// Set status error description from exception events.
    #[serde(default = "crate::util::default_true")]
    status_from_error_events: bool,
    /// Track both busy and inactive times for spans.
    #[serde(default)]
    inactivity: bool,
    /// Record thread name/ID in span attributes.
    #[serde(default = "crate::util::default_true")]
    thread_info: bool,
}

impl Default for TracingIncludes {
    fn default() -> Self {
        Self {
            location: true,
            exception_from_error_fields: true,
            exception_from_error_events: true,
            status_from_error_events: true,
            inactivity: false,
            thread_info: true,
        }
    }
}

/// Batch span processor configuration.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[non_exhaustive]
struct TracingBatchConfig {
    /// The maximum queue size to buffer spans for delayed processing.
    ///
    /// If the queue gets full it drops the spans. The default value of is 2048.
    #[serde(default = "TracingBatchConfig::default_max_queue_size")]
    max_queue_size: NonZeroUsize,
    /// The delay interval between two consecutive processing of batches.
    ///
    /// The default value is 5 seconds.
    #[serde(default = "TracingBatchConfig::default_scheduled_delay")]
    scheduled_delay: Duration,
    /// The maximum number of spans to process in a single batch.
    ///
    /// If there are more than one batch worth of spans then it processes multiple batches
    /// of spans one batch after the other without any delay. The default value is 512.
    #[serde(default = "TracingBatchConfig::default_max_export_batch_size")]
    max_export_batch_size: NonZeroUsize,
}

impl Default for TracingBatchConfig {
    fn default() -> Self {
        Self {
            max_queue_size: Self::default_max_queue_size(),
            scheduled_delay: Self::default_scheduled_delay(),
            max_export_batch_size: Self::default_max_export_batch_size(),
        }
    }
}

impl TracingBatchConfig {
    /// Default value for [`Self::max_queue_size`].
    #[must_use]
    #[inline]
    fn default_max_queue_size() -> NonZeroUsize {
        // SAFETY: 2048 is always non-zero
        NonZeroUsize::new(2048).unwrap()
    }

    /// Default value for [`Self::scheduled_delay`].
    #[must_use]
    #[inline]
    fn default_scheduled_delay() -> Duration {
        Duration::from_secs(5)
    }

    /// Default value for [`Self::max_export_batch_size`].
    #[must_use]
    #[inline]
    fn default_max_export_batch_size() -> NonZeroUsize {
        // SAFETY: 512 is always non-zero
        NonZeroUsize::new(512).unwrap()
    }

    /// Create OpenTelemetry batch config builder.
    #[must_use]
    fn to_builder(&self) -> BatchConfigBuilder {
        BatchConfigBuilder::default()
            .with_max_queue_size(self.max_queue_size.get())
            .with_scheduled_delay(self.scheduled_delay)
            .with_max_export_batch_size(self.max_export_batch_size.get())
    }
}
