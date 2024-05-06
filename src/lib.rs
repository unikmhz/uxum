#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(elided_lifetimes_in_paths, unreachable_pub)]
// #![warn(clippy::pedantic)]
// #![warn(clippy::restriction)]
// #![warn(clippy::cargo)]
// #![warn(missing_docs)]
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
mod layers;
mod logging;
mod metrics;
mod notify;
pub mod prelude;
pub mod reexport;
mod response;
mod signal;
mod telemetry;
mod tracing;
mod util;

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
    layers::{
        buffer::HandlerBufferConfig,
        ext::{Deadline, HandlerName},
        rate::RateLimitError,
        timeout::HandlerTimeoutsConfig,
    },
    logging::LoggingConfig,
    metrics::{MetricsBuilder, MetricsError, MetricsState},
    notify::ServiceNotifier,
    response::{GetResponseSchemas, ResponseSchema},
    signal::{SignalError, SignalStream},
    telemetry::OpenTelemetryConfig,
    tracing::TracingConfig,
    util::ResponseExtension,
};
