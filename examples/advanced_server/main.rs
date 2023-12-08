use std::{net::SocketAddr, time::Duration};

use config::{Config, File};
use serde::Deserialize;
use uxum::{
    handler,
    reexport::{
        axum::extract::{ConnectInfo, Path, Query},
        schemars::JsonSchema,
        tracing,
        tracing_subscriber::util::SubscriberInitExt,
    },
    ApiDocBuilder, AppBuilder, AppConfig, ServerBuilder,
};

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
    let api_doc = ApiDocBuilder::default()
        .with_app_title("Advanced Server")
        .with_tag("tag1", Some("Some tag"), Some("http://example.com/tag1"))
        .with_tag("tag2", Some("Some other tag"), None::<&str>);
    let app_builder: AppBuilder = config.app.into();
    config
        .server
        .build()
        .await
        .unwrap()
        .serve(
            app_builder
                .with_app_name("advanced_server")
                .with_app_version("1.2.3")
                .with_api_doc(api_doc)
                .build()
                .unwrap()
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
}

/// Greet the Axum world
#[handler(
    name = "hello_world",
    path = "/",
    method = "GET",
    spec(
        docs(description = "Some link", url = "http://example.com/hello_world"),
        tag = "tag1",
        tag = "tag2"
    )
)]
async fn root_handler() -> &'static str {
    tracing::info!("Said hello to the Axum world");
    "Hello Axum world!"
}

/// Sleep for some time and return response
#[handler]
async fn sleep(ConnectInfo(client): ConnectInfo<SocketAddr>) -> String {
    tokio::time::sleep(Duration::from_secs(3)).await;
    format!("Hello {client}! Woken up after 3 seconds!")
}

/// Query parameters
#[derive(Deserialize, JsonSchema)]
struct QueryName {
    /// Name of the person to greet
    #[serde(default = "QueryName::default_name")]
    name: String,
}

impl QueryName {
    fn default_name() -> String {
        "Jebediah".into()
    }
}

/// Greet someone using a name from a query string
#[handler]
async fn name_from_qs(q: Query<QueryName>) -> String {
    format!("Hello {}!", q.name)
}

/// Greet someone using a name from a URL path element
#[handler(path = "/hello/:name")]
async fn name_from_path(args: Path<String>) -> String {
    format!("Hello {}!", args.0)
}
