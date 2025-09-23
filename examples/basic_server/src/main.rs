//! Very basic example of using uxum builders to quickly set up a service.
//! No configuration is provided.

use uxum::{
    prelude::*,
    reexport::{tokio, tracing_subscriber},
};

/// Application entry point.
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let app_builder = AppBuilder::default();
    let app = app_builder.build().expect("Unable to build app");
    ServerBuilder::new()
        .build()
        .await
        .expect("Unable to build server")
        .serve(app.into_make_service())
        .await
        .expect("Server error");
}

/// Sample request handler.
#[handler(
    name = "hello_world",
    path = "/hello",
    method = "GET",
    tags = ["tag1", "tag2"]
)]
async fn hello_handler() -> &'static str {
    "Hello Axum world!"
}

/// Bare request handler.
///
/// This handler has no metadata, so everything is deduced from function signature.
/// Handler name is taken from function name.
/// Path is composed by prepending '/' to handler name.
/// Method is POST if some request body extractor is provided, otherwise it is GET.
#[handler]
async fn other() -> &'static str {
    "Another handler"
}
