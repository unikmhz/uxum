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
mod builder;
mod config;
mod errors;
mod layers;
mod logging;
mod metrics;
mod otel;
pub mod prelude;
pub mod reexport;
mod response;
mod tracing;
mod util;

pub use uxum_macros::handler;

pub use self::{
    apidoc::{ApiDocBuilder, ApiDocError},
    builder::{
        app::{apply_layers, AppBuilder, AppBuilderError, HandlerExt},
        server::{ServerBuilder, ServerBuilderError},
    },
    config::*,
    layers::{ext::HandlerName, rate::RateLimitError},
    metrics::{MetricsBuilder, MetricsError, MetricsState},
    response::{GetResponseSchemas, ResponseSchema},
    util::ResponseExtension,
};
