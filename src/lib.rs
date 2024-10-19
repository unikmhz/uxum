#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(elided_lifetimes_in_paths, unreachable_pub)]
// #![warn(clippy::pedantic)]
// #![warn(clippy::restriction)]
// #![warn(clippy::cargo)]
#![warn(
    missing_docs,
    clippy::doc_link_with_quotes,
    clippy::doc_markdown,
    clippy::missing_errors_doc
)]
// #![allow(clippy::module_name_repetitions)]
// #![allow(clippy::single_call_fn)]
// #![allow(clippy::implicit_return)]
// #![allow(clippy::std_instead_of_core)]
// #![allow(clippy::float_arithmetic)]
// #![allow(clippy::question_mark_used)]
// #![allow(clippy::pattern_type_mismatch)]
// #![allow(clippy::multiple_unsafe_ops_per_block)]
// #![allow(clippy::absolute_paths)]
// #![allow(clippy::needless_pass_by_value)]
// #![allow(clippy::missing_trait_methods)]

mod apidoc;
mod auth;
mod builder;
mod config;
mod errors;
mod handle;
mod http_client;
mod layers;
mod logging;
mod metrics;
mod notify;
pub mod prelude;
mod probes;
pub mod reexport;
mod response;
mod runtime;
mod signal;
pub mod state;
mod telemetry;
mod tracing;
mod util;
mod watchdog;

pub use uxum_macros::handler;

pub use self::{
    apidoc::{ApiDocBuilder, ApiDocError},
    auth::*,
    builder::{
        app::{AppBuilder, AppBuilderError, HandlerExt},
        server::{
            Http1Config, Http2Config, Http2KeepaliveConfig, IpConfig, ServerBuilder,
            ServerBuilderError, TcpConfig, TcpKeepaliveConfig,
        },
    },
    config::*,
    handle::{Handle, HandleError},
    http_client::*,
    layers::{
        buffer::HandlerBufferConfig,
        cors::CorsConfig,
        ext::{Deadline, HandlerName},
        rate::{HandlerRateLimitConfig, RateLimitError},
        request_id::CURRENT_REQUEST_ID,
        timeout::{HandlerTimeoutConfig, TimeoutError, CURRENT_DEADLINE},
    },
    logging::LoggingConfig,
    metrics::{MetricsBuilder, MetricsError, MetricsState},
    notify::ServiceNotifier,
    probes::{ProbeConfig, ProbeState},
    response::{GetResponseSchemas, ResponseSchema},
    runtime::RuntimeConfig,
    signal::{SignalError, SignalStream},
    telemetry::OpenTelemetryConfig,
    tracing::TracingConfig,
    util::ResponseExtension,
    watchdog::WatchdogConfig,
};
