//! Instrumented HTTP client.
//!
//! Uses [`reqwest`] internally.

mod cb;
mod config;
mod errors;
mod middleware;

pub use self::{config::HttpClientConfig, errors::HttpClientError};
