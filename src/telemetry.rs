use std::time::Duration;

use opentelemetry::KeyValue;
use opentelemetry_sdk::{
    resource::{
        EnvResourceDetector, OsResourceDetector, ProcessResourceDetector,
        SdkProvidedResourceDetector, TelemetryResourceDetector,
    },
    trace::Tracer,
    Resource,
};
use opentelemetry_semantic_conventions::resource as res;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::AppConfig;

/// Common OpenTelemetry configuration
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct OpenTelemetryConfig {
    /// OpenTelemetry resource detection timeout
    #[serde(
        default = "OpenTelemetryConfig::default_detector_timeout",
        with = "humantime_serde"
    )]
    pub detector_timeout: Duration,
}

impl Default for OpenTelemetryConfig {
    fn default() -> Self {
        Self {
            detector_timeout: Self::default_detector_timeout(),
        }
    }
}

impl OpenTelemetryConfig {
    /// Default value for [`Self::detector_timeout`]
    fn default_detector_timeout() -> Duration {
        Duration::from_secs(6)
    }
}

/// Guard for logging and tracing subsystems
///
/// Unwritten logs will be flushed when dropping this object. This might help even in case of a
/// panic.
#[allow(dead_code)]
pub struct TelemetryGuard {
    /// Guards for [`tracing_appender::non_blocking::NonBlocking`]
    buf_guards: Vec<WorkerGuard>,
    /// Tracing pipeline
    tracer: Option<Tracer>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.tracer.as_ref().and_then(|t| t.provider()) {
            for res in provider.force_flush() {
                if let Err(err) = res {
                    eprintln!("Error flushing spans: {err}");
                }
            }
        }
    }
}

/// Error type returned on telemetry initialization
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TelemetryError {
    /// Error while setting up logging
    #[error(transparent)]
    Logging(#[from] crate::logging::LoggingError),
    /// Error while setting up trace collection and propagation
    #[error(transparent)]
    Tracing(#[from] crate::tracing::TracingError),
}

impl AppConfig {
    /// Initialize logging and tracing subsystems
    ///
    /// Returns a guard that shouldn't be dropped as long as there is a need for these subsystems.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any part of initializing of tracing or logging subsystems ends with and
    /// error.
    pub fn init_telemetry(&mut self) -> Result<TelemetryGuard, TelemetryError> {
        let (registry, buf_guards) = self.logging.make_registry()?;
        let otel_res = self.otel_resource();
        let tracer = if let Some(tcfg) = self.tracing.as_mut() {
            let tracer = tcfg.build_pipeline(otel_res)?;
            let layer = tcfg.build_layer(&tracer);
            registry.with(layer).init();
            opentelemetry::global::set_text_map_propagator(
                opentelemetry_sdk::propagation::TraceContextPropagator::default(),
            );
            Some(tracer)
        } else {
            registry.init();
            None
        };
        Ok(TelemetryGuard { buf_guards, tracer })
    }

    /// Get OpenTelemetry resource
    ///
    /// Creates new resource object on first call. On subsequent calls returns previously created
    /// object as a clone.
    #[must_use]
    pub fn otel_resource(&mut self) -> Resource {
        if let Some(res) = &self.otel_res {
            return res.clone();
        }
        let mut resource = Resource::from_detectors(
            self.otel.detector_timeout,
            vec![
                Box::new(OsResourceDetector),
                Box::new(ProcessResourceDetector),
                Box::new(SdkProvidedResourceDetector),
                Box::new(EnvResourceDetector::new()),
                Box::new(TelemetryResourceDetector),
            ],
        );
        // TODO: res::SERVICE_NAMESPACE
        // TODO: res::DEPLOYMENT_ENVIRONMENT
        let mut static_resources = Vec::new();
        if let Some(val) = &self.app_name {
            static_resources.push(KeyValue::new(res::SERVICE_NAME, val.clone()));
        }
        if let Some(val) = &self.app_version {
            static_resources.push(KeyValue::new(res::SERVICE_VERSION, val.clone()));
        }
        if !static_resources.is_empty() {
            resource = resource.merge(&mut Resource::new(static_resources));
        }
        self.otel_res = Some(resource.clone());
        resource
    }
}
