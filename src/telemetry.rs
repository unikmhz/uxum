//! Global OpenTelemetry setup code.

use std::collections::HashMap;

use opentelemetry::KeyValue;
use opentelemetry_otlp::Protocol;
use opentelemetry_resource_detectors::{
    HostResourceDetector, K8sResourceDetector, OsResourceDetector, ProcessResourceDetector,
};
use opentelemetry_sdk::{
    Resource,
    resource::{
        EnvResourceDetector, ResourceDetector, SdkProvidedResourceDetector,
        TelemetryResourceDetector,
    },
};
use opentelemetry_semantic_conventions::resource as res;
use serde::{Deserialize, Serialize};

use crate::{config::AppConfig, util::env::parse_env_vars};

/// Configuration to enable/disable various detectors that populate OpenTelemetry resource
/// attributes.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[non_exhaustive]
pub struct TelemetryDetectorsConfig {
    /// Run [`HostResourceDetector`]. Enabled by default.
    /// Can detect following attributes:
    /// * `host.id`
    /// * `host.arch`
    #[serde(default = "crate::util::default_true")]
    pub host: bool,
    /// Run [`K8sResourceDetector`]. Enabled by default.
    /// Can detect following attributes:
    /// * `k8s.pod.name`
    /// * `k8s.namespace.name`
    #[serde(default = "crate::util::default_true")]
    pub k8s: bool,
    /// Run [`OsResourceDetector`]. Enabled by default.
    /// Can detect following attributes:
    /// * `os.type`
    #[serde(default = "crate::util::default_true")]
    pub os: bool,
    /// Run [`ProcessResourceDetector`]. Enabled by default.
    /// Can detect following attributes:
    /// * `process.command_args`
    /// * `process.pid`
    /// * `process.runtime.version`
    /// * `process.runtime.name`
    /// * `process.runtime.description`
    #[serde(default = "crate::util::default_true")]
    pub process: bool,
    /// Run [`SdkProvidedResourceDetector`]. Disabled by default.
    /// Can detect following attributes:
    /// * `service.name`
    #[serde(default)]
    pub provided: bool,
    /// Run [`EnvResourceDetector`]. Enabled by default.
    /// Can inject attributes from the environment, based on `OTEL_RESOURCE_ATTRIBUTES` environment
    /// variable.
    #[serde(default = "crate::util::default_true")]
    pub env: bool,
    /// Run [`TelemetryResourceDetector`]. Enabled by default.
    /// Provides following attributes:
    /// * `telemetry.sdk.name`
    /// * `telemetry.sdk.language`
    /// * `telemetry.sdk.version`
    #[serde(default = "crate::util::default_true")]
    pub sdk: bool,
}

impl Default for TelemetryDetectorsConfig {
    fn default() -> Self {
        Self {
            host: true,
            k8s: true,
            os: true,
            process: true,
            provided: false,
            env: true,
            sdk: true,
        }
    }
}

impl TelemetryDetectorsConfig {
    /// Build list of detector objects for use in resource builder.
    fn detector_list(&self) -> Vec<Box<dyn ResourceDetector>> {
        let mut detectors: Vec<Box<dyn ResourceDetector>> = Vec::new();
        if self.host {
            detectors.push(Box::new(HostResourceDetector::default()));
        }
        if self.k8s {
            detectors.push(Box::new(K8sResourceDetector));
        }
        if self.os {
            detectors.push(Box::new(OsResourceDetector));
        }
        if self.process {
            detectors.push(Box::new(ProcessResourceDetector));
        }
        if self.provided {
            detectors.push(Box::new(SdkProvidedResourceDetector));
        }
        if self.env {
            detectors.push(Box::new(EnvResourceDetector::new()));
        }
        if self.sdk {
            detectors.push(Box::new(TelemetryResourceDetector));
        }
        detectors
    }
}

/// OpenTelemetry configuration common for logging, metrics and tracing.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[non_exhaustive]
pub struct TelemetryConfig {
    /// Static labels to add to gathered metrics.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub labels: HashMap<String, String>,
    /// Parse environment variable references in label values.
    #[serde(default)]
    pub parse_labels: bool,
    /// Configure various OpenTelemetry resource detectors to (not) run.
    #[serde(default)]
    pub detectors: TelemetryDetectorsConfig,
}

impl TelemetryConfig {
    /// Add one static label to be added to gathered metrics.
    #[must_use]
    pub fn with_label<T, U>(&mut self, key: T, value: U) -> &mut Self
    where
        T: ToString,
        U: ToString,
    {
        self.labels.insert(key.to_string(), value.to_string());
        self
    }

    /// Add multiple static labels to be added to gathered metrics.
    #[must_use]
    pub fn with_labels<'a, T, U, V>(&mut self, kvs: V) -> &mut Self
    where
        T: ToString + 'a,
        U: ToString + 'a,
        V: IntoIterator<Item = (&'a T, &'a U)>,
    {
        self.labels.extend(
            kvs.into_iter()
                .map(|(key, val)| (key.to_string(), val.to_string())),
        );
        self
    }

    pub fn static_resources(&self) -> impl Iterator<Item = KeyValue> + '_ {
        self.labels.iter().map(|(key, val)| {
            KeyValue::new(
                key.clone(),
                match self.parse_labels {
                    true => parse_env_vars(val).into_owned(),
                    false => val.clone(),
                },
            )
        })
    }
}

impl AppConfig {
    /// Get OpenTelemetry resource.
    ///
    /// Creates new resource object on first call. On subsequent calls returns previously created
    /// object as a clone.
    #[must_use]
    pub fn otel_resource(&self) -> Resource {
        // TODO: res::SERVICE_NAMESPACE.
        // TODO: res::DEPLOYMENT_ENVIRONMENT.
        let mut static_resources: Vec<_> = self.telemetry.static_resources().collect();
        if let Some(val) = &self.app_name {
            static_resources.push(KeyValue::new(res::SERVICE_NAME, val.clone()));
        }
        if let Some(val) = &self.app_version {
            static_resources.push(KeyValue::new(res::SERVICE_VERSION, val.clone()));
        }
        let detectors = self.telemetry.detectors.detector_list();
        Resource::builder()
            .with_detectors(&detectors)
            .with_attributes(static_resources)
            .build()
    }
}

/// OTLP protocol variant to use for data export.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum OtlpProtocol {
    /// GRPC over HTTP.
    #[default]
    Grpc,
    /// Protobuf over HTTP.
    HttpBinary,
    /// JSON over HTTP.
    HttpJson,
}

impl From<OtlpProtocol> for Protocol {
    fn from(value: OtlpProtocol) -> Self {
        match value {
            OtlpProtocol::Grpc => Self::Grpc,
            OtlpProtocol::HttpBinary => Self::HttpBinary,
            OtlpProtocol::HttpJson => Self::HttpJson,
        }
    }
}
