//! Global OpenTelemetry setup code.

use std::time::Duration;

use opentelemetry::KeyValue;
use opentelemetry_resource_detectors::{OsResourceDetector, ProcessResourceDetector};
use opentelemetry_sdk::{
    resource::{EnvResourceDetector, SdkProvidedResourceDetector, TelemetryResourceDetector},
    Resource,
};
use opentelemetry_semantic_conventions::resource as res;
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;

/// Common OpenTelemetry configuration.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct OpenTelemetryConfig {
    /// OpenTelemetry resource detection timeout.
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
    /// Default value for [`Self::detector_timeout`].
    fn default_detector_timeout() -> Duration {
        Duration::from_secs(6)
    }
}

impl AppConfig {
    /// Get OpenTelemetry resource.
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
        // TODO: res::SERVICE_NAMESPACE.
        // TODO: res::DEPLOYMENT_ENVIRONMENT.
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
