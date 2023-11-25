//! Absolutely minimal example using uxum framework.
//! This does not use AppBuilder, only ServerBuilder.

use uxum::{
    reexport::axum::{routing::get, Router},
    ServerBuilder,
};

#[tokio::main]
async fn main() {
    let app = Router::new().route("/", get(root_handler));
    ServerBuilder::new()
        .build()
        .await
        .unwrap()
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn root_handler() -> &'static str {
    "Hello world!"
}
