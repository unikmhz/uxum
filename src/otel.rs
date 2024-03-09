use std::time::Duration;

use opentelemetry::KeyValue;
use opentelemetry_sdk::{
    resource::{
        EnvResourceDetector, OsResourceDetector, ProcessResourceDetector,
        SdkProvidedResourceDetector, TelemetryResourceDetector,
    },
    Resource,
};
use opentelemetry_semantic_conventions::resource as res;
use serde::{Deserialize, Serialize};

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

pub(crate) fn otel_resource(
    timeout: Duration,
    app_namespace: Option<impl ToString>,
    app_name: Option<impl ToString>,
    app_version: Option<impl ToString>,
) -> Resource {
    let mut resource = Resource::from_detectors(
        timeout,
        vec![
            Box::new(OsResourceDetector),
            Box::new(ProcessResourceDetector),
            Box::new(SdkProvidedResourceDetector),
            Box::new(EnvResourceDetector::new()),
            Box::new(TelemetryResourceDetector),
        ],
    );
    let mut static_resources = Vec::new();
    if let Some(val) = app_namespace {
        static_resources.push(KeyValue::new(res::SERVICE_NAMESPACE, val.to_string()));
    }
    if let Some(val) = app_name {
        static_resources.push(KeyValue::new(res::SERVICE_NAME, val.to_string()));
    }
    if let Some(val) = app_version {
        static_resources.push(KeyValue::new(res::SERVICE_VERSION, val.to_string()));
    }
    if !static_resources.is_empty() {
        resource = resource.merge(&mut Resource::new(static_resources));
    }
    resource
}
