//! Centralized place to re-export dependency crates

pub use askama;
pub use axum;
pub use axum_server;
pub use bytes;
pub use config;
pub use http;
pub use hyper;
pub use inventory;
pub use mime;
pub use okapi::{self, openapi3, schemars};
pub use opentelemetry;
pub use problemdetails;
#[cfg(feature = "grpc")]
pub use prost;
pub use reqwest;
pub use reqwest_middleware;
pub use tokio;
#[cfg(feature = "grpc")]
pub use tonic;
pub use tower;
pub use tower_http;
pub use tracing;
pub use tracing_subscriber;
