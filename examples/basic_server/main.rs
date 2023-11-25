//! Very basic example of using uxum builders to quickly set up a service.
//! No configuration is provided.

use uxum::{handler, AppBuilder, AppConfig, ServerBuilder};

#[tokio::main]
async fn main() {
    // TODO: remove init
    tracing_subscriber::fmt::init();
    let app_builder: AppBuilder = AppConfig::default().into();
    ServerBuilder::new()
        .build()
        .await
        .unwrap()
        .serve(app_builder.build().into_make_service())
        .await
        .unwrap();
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
