#![doc = include_str!("../README.md")]
#![cfg_attr(not(test), forbid(unsafe_code))]
#![deny(elided_lifetimes_in_paths, unreachable_pub)]
#![warn(
    missing_docs,
    clippy::doc_link_with_quotes,
    clippy::doc_markdown,
    clippy::missing_errors_doc
)]
#![cfg_attr(test, deny(warnings))]

mod apidoc;
mod auth;
mod behavior;
mod builder;
mod config;
pub mod crypto;
mod errors;
mod handle;
mod http_client;
#[cfg(feature = "kafka")]
mod kafka;
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
#[cfg(feature = "spiffe")]
pub mod spiffe;
pub mod state;
mod telemetry;
mod tracing;
mod util;
mod watchdog;

pub use uxum_macros::handler;

pub use self::{
    apidoc::{ApiDocBuilder, ApiDocError},
    auth::*,
    behavior::{AppBehavior, StandardAppBehavior},
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
        cors::CorsConfig,
        ext::{Deadline, HandlerName},
        rate::{HandlerRateLimitConfig, RateLimitError},
        request_id::CURRENT_REQUEST_ID,
        timeout::{CURRENT_DEADLINE, HandlerTimeoutConfig, TimeoutError},
    },
    logging::LoggingConfig,
    metrics::{
        AdditionalMetricLabels, MetricsBuilder, MetricsError, MetricsState, text_exporter::*,
    },
    notify::ServiceNotifier,
    probes::{ProbeConfig, ProbeState},
    response::{GetResponseSchemas, ResponseSchema},
    runtime::{RuntimeConfig, RuntimeError},
    signal::{SignalError, SignalStream},
    tracing::TracingConfig,
    util::{OptVec, ResponseExtension},
    watchdog::WatchdogConfig,
};

#[cfg(feature = "kafka")]
pub use self::kafka::{KafkaLogAppender, KafkaProducerConfig, LogProducerContext};

#[cfg(feature = "spiffe")]
pub use self::spiffe::{SpiffeConfig, SpiffeError};
