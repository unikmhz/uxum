use config::{Config, File};
use serde::Deserialize;
use tracing_subscriber::util::SubscriberInitExt;
use uxum::{handler, AppBuilder, AppConfig, ServerBuilder};

#[derive(Deserialize)]
struct ServiceConfig {
    #[serde(flatten)]
    app: AppConfig,
    server: ServerBuilder,
}

#[tokio::main]
async fn main() {
    let config: ServiceConfig = Config::builder()
        .add_source(File::with_name("examples/advanced_server/config.yaml"))
        .build()
        .unwrap()
        .try_deserialize()
        .unwrap();
    let (registry, _buf_guards) = config.app.logging.make_registry().unwrap();
    registry.init();
    let app_builder: AppBuilder = config.app.into();
    config
        .server
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
