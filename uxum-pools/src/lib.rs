#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(elided_lifetimes_in_paths, unreachable_pub)]
#![warn(
    missing_docs,
    clippy::doc_link_with_quotes,
    clippy::doc_markdown,
    clippy::missing_errors_doc
)]

#[cfg(feature = "async")]
pub mod r#async;
pub mod error;
mod metrics;
mod resource;
pub mod sync;

pub use crate::resource::InstrumentedResource;
