use uxum::{handler, AppBuilder, ServerBuilder};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    ServerBuilder::new()
        .build()
        .await
        .unwrap()
        .serve(AppBuilder::build().into_make_service())
        .await
        .unwrap();
}

#[handler(
    name = "hello_world",
    path = "/",
    method = "GET",
    spec(tag = "tag1", tag = "tag2",)
)]
async fn root_handler() -> &'static str {
    "Hello Axum world!"
}
