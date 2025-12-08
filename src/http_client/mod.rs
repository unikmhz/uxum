//! Instrumented HTTP client.
//!
//! Uses [`reqwest`] internally.

mod cb;
mod config;
mod errors;
mod metrics;
mod middleware;
mod tracing;

pub use self::{
    cb::{CircuitBreakerMiddleware, CircuitBreakerRejection},
    config::HttpClientConfig,
    errors::HttpClientError,
    metrics::MetricsMiddleware,
    tracing::{DisableOtelPropagation, TracingMiddleware},
};
