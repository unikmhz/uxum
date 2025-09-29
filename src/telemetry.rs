//! Global OpenTelemetry setup code.

use std::collections::HashMap;

use opentelemetry::KeyValue;
use opentelemetry_otlp::Protocol;
use opentelemetry_resource_detectors::{OsResourceDetector, ProcessResourceDetector};
use opentelemetry_sdk::{
    resource::{EnvResourceDetector, SdkProvidedResourceDetector, TelemetryResourceDetector},
    Resource,
};
use opentelemetry_semantic_conventions::resource as res;
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;

/// OpenTelemetry configuration common for logging, metrics and tracing.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[non_exhaustive]
pub struct TelemetryConfig {
    /// Static labels to add to gathered metrics.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    labels: HashMap<String, String>,
    /// TODO: parse $VARIABLE in label values.
    #[serde(default)]
    parse_labels: bool,
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
        self.labels
            .iter()
            .map(|(key, val)| KeyValue::new(key.clone(), val.clone()))
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
        Resource::builder()
            .with_detectors(&[
                Box::new(OsResourceDetector),
                Box::new(ProcessResourceDetector),
                Box::new(SdkProvidedResourceDetector),
                Box::new(EnvResourceDetector::new()),
                Box::new(TelemetryResourceDetector),
            ])
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
