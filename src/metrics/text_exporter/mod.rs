//! OpenTelemetry Prometheus Text Exporter.
//!
//! Local fork of <https://github.com/sandhose/opentelemetry-prometheus-text-exporter>
//! With `resource_selector` patch.

pub(crate) mod exporter;
pub(crate) mod resource_selector;
pub(crate) mod serialize;

pub use self::{
    exporter::{ExporterBuilder, PrometheusExporter},
    resource_selector::ResourceSelector,
};
