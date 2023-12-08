#![forbid(unsafe_code)]
#![deny(elided_lifetimes_in_paths)]
#![deny(unreachable_pub)]

mod apidoc;
mod builder;
mod config;
mod errors;
mod layers;
mod logging;
mod metrics;
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
};
