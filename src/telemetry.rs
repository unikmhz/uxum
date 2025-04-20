//! Global OpenTelemetry setup code.

use opentelemetry::KeyValue;
use opentelemetry_resource_detectors::{OsResourceDetector, ProcessResourceDetector};
use opentelemetry_sdk::{
    resource::{EnvResourceDetector, SdkProvidedResourceDetector, TelemetryResourceDetector},
    Resource,
};
use opentelemetry_semantic_conventions::resource as res;

use crate::config::AppConfig;

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
        // TODO: res::SERVICE_NAMESPACE.
        // TODO: res::DEPLOYMENT_ENVIRONMENT.
        let mut static_resources = Vec::new();
        if let Some(val) = &self.app_name {
            static_resources.push(KeyValue::new(res::SERVICE_NAME, val.clone()));
        }
        if let Some(val) = &self.app_version {
            static_resources.push(KeyValue::new(res::SERVICE_VERSION, val.clone()));
        }
        let resource = Resource::builder()
            .with_detectors(&[
                Box::new(OsResourceDetector),
                Box::new(ProcessResourceDetector),
                Box::new(SdkProvidedResourceDetector),
                Box::new(EnvResourceDetector::new()),
                Box::new(TelemetryResourceDetector),
            ])
            .with_attributes(static_resources)
            .build();
        self.otel_res = Some(resource.clone());
        resource
    }
}
