use std::net::SocketAddr;

use config::{Config, File};
use serde::Deserialize;
use uxum::prelude::*;

/// Root container for app configuration
#[derive(Deserialize)]
struct ServiceConfig {
    /// Application configuration
    #[serde(flatten)]
    app: AppConfig,
    /// Server configuration
    server: ServerBuilder,
}

/// Application entry point
#[tokio::main]
async fn main() {
    // Load configuration from file
    let mut config: ServiceConfig = Config::builder()
        .add_source(File::with_name("examples/inner_service/config.yaml"))
        .build()
        .expect("Unable to load configuration")
        .try_deserialize()
        .expect("Error deserializing configuration");
    // Add some hard-coded values to [`AppConfig`]
    let app_cfg = config
        .app
        .with_app_name("inner_service")
        .with_app_version("2.3.4");
    // Initialize uxum handle, including logging and tracing
    //
    // Logging will start working right after this call, and until the returned
    // guard is dropped.
    let _uxum_handle = app_cfg.handle().expect("Error initializing handle");
    // Create app builder from app config
    //
    // Also enable the auth subsystem.
    let mut app_builder = AppBuilder::from_config(app_cfg);
    // Some hard-coded parameters for built-in API documentation
    app_builder.configure_api_doc(|api_doc| {
        api_doc
            .with_app_title("Inner Service")
            .with_description("Inner service for testing distributed tracing.")
            .with_contact_name("Uxum developers")
            .with_contact_url("http://uxum.example.com")
            .with_contact_email("example@example.com")
    });
    // Build main application router
    let app = app_builder.build().expect("Unable to build app");
    // Create server handle
    let handle = axum_server::Handle::new();
    // Spawn signal handler
    config
        .server
        .spawn_signal_handler(handle.clone())
        .expect("Unable to spawn signal handler");
    // Build server, link the handle and run the app
    config
        .server
        .build()
        .await
        .expect("Unable to build server")
        .handle(handle)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .expect("Server error");
}

#[handler]
async fn inner() -> String {
    "w00t!".into()
}
