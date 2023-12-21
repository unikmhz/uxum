#![forbid(unsafe_code)]
#![deny(elided_lifetimes_in_paths)]
#![deny(unreachable_pub)]
// #![warn(missing_docs)]

mod apidoc;
mod builder;
mod config;
mod errors;
mod layers;
mod logging;
mod metrics;
pub mod prelude;
pub mod reexport;
mod util;

pub use uxum_handler_macros::handler;

pub use self::{
    apidoc::{ApiDocBuilder, ApiDocError},
    builder::{
        app::{apply_layers, AppBuilder, AppBuilderError, HandlerExt},
        server::{ServerBuilder, ServerBuilderError},
    },
    config::*,
    layers::{ext::HandlerName, rate::RateLimitError},
    metrics::{MetricsBuilder, MetricsError, MetricsState},
    util::ResponseExtension,
};
