//! Very basic example of using uxum builders to quickly set up a service.
//! No configuration is provided.

use uxum::prelude::*;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let app_builder = AppBuilder::default();
    let (app, _tracer) = app_builder.build().expect("Unable to build app");
    ServerBuilder::new()
        .build()
        .await
        .expect("Unable to build server")
        .serve(app.into_make_service())
        .await
        .expect("Server error");
}

#[handler(
    name = "hello_world",
    path = "/",
    method = "GET",
    spec(tag = "tag1", tag = "tag2")
)]
async fn root_handler() -> &'static str {
    "Hello Axum world!"
}

// This handler has no metadata, so everything is deduced from function signature.
// Handler name is taken from function name.
// Path is composed by prepending '/' to handler name.
// Method is always GET (FIXME).
#[handler]
async fn other() -> &'static str {
    "Another handler"
}
