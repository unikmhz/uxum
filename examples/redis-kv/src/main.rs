//! More realistic example of a simple service using UXUM framework.
//! Also utilizes [`bb8`] and [`uxum_pools`] for Redis connection pooling.

use std::{net::SocketAddr, ops::Deref, sync::Arc, time::Duration};

use bb8_redis::{bb8, redis::AsyncCommands, RedisConnectionManager};
use clap::Parser;
use serde::{Deserialize, Serialize};
use uxum::{prelude::*, GetResponseSchemas, ResponseSchema};
use uxum_pools::r#async::InstrumentedPool;

/// Command-line arguments.
#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to configuration file.
    #[arg(
        short,
        long,
        value_name = "FILE",
        value_parser = validate_config_path,
        default_value = default_config_path(),
        env = "REDIS_KV_CONFIG_FILE"
    )]
    config_file: String,
}

/// Default path for configuration file.
fn default_config_path() -> &'static str {
    "examples/redis-kv/config.yaml"
}

/// Sanitize provided configuration file path.
fn validate_config_path(v: &str) -> Result<String, String> {
    let path = v.to_string();
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => return Err(format!("Unable to check file: {e}")),
    };
    if !meta.is_file() {
        return Err("Configuration is not a file".into());
    }
    Ok(path)
}

/// Redis connection pool configuration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct LocalRedisConfig {
    /// Redis URL to connect to.
    #[serde(default = "LocalRedisConfig::default_url")]
    url: String,
    /// Maximum size of Redis connection pool.
    #[serde(default = "LocalRedisConfig::default_max_size")]
    max_size: u32,
    /// Minimum idle connections to keep in the pool.
    #[serde(default)]
    min_idle: Option<u32>,
    /// Inactivity period before we consider a connection as idle.
    #[serde(default, with = "humantime_serde")]
    idle_timeout: Option<Duration>,
}

impl LocalRedisConfig {
    /// Default value for [`Self::url`].
    #[must_use]
    #[inline]
    fn default_url() -> String {
        String::from("redis://localhost")
    }

    /// Default value for [`Self::max_size`].
    #[must_use]
    #[inline]
    fn default_max_size() -> u32 {
        8
    }
}

/// Service-specific configuration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct LocalConfig {
    /// Redis configuration.
    redis: LocalRedisConfig,
}

/// Application-wide state object.
#[derive(Clone, Debug)]
pub struct AppState(Arc<AppStateInner>);

impl Deref for AppState {
    type Target = AppStateInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Internal shared state object.
#[derive(Debug)]
pub struct AppStateInner {
    #[allow(dead_code)]
    args: Args,
    pool: InstrumentedPool<bb8::Pool<RedisConnectionManager>>,
}

/// Application handler error type.
#[derive(Debug, JsonSchema, Serialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum ApiError {
    #[error("Redis error: {0}")]
    Redis(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let mut resp = Json::from(self).into_response();
        *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        resp.headers_mut().insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static(mime::APPLICATION_JSON.as_ref()),
        );
        resp
    }
}

impl GetResponseSchemas for ApiError {
    type ResponseIter = [ResponseSchema; 1];
    fn get_response_schemas(gen: &mut schemars::gen::SchemaGenerator) -> Self::ResponseIter {
        [ResponseSchema {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            response: openapi3::Response {
                description: "Error response".into(),
                content: okapi::map! {
                    mime::APPLICATION_JSON.to_string() => openapi3::MediaType {
                        schema: Some(gen.subschema_for::<Self>().into_object()),
                        ..Default::default()
                    },
                },
                ..Default::default()
            },
        }]
    }
}

/// Application entry point.
fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();
    eprintln!("CLI args: {args:?}");
    // Merge, load and deserialize configuration.
    let mut config = ServiceConfig::<LocalConfig>::builder()
        .with_file(&args.config_file)
        .with_env("REDIS_KV")
        .build()?;
    // Add some hard-coded values to [`AppConfig`].
    let app_cfg = config
        .app
        .with_app_name(env!("CARGO_PKG_NAME"))
        .with_app_version(env!("CARGO_PKG_VERSION"));
    // Build and start Tokio runtime.
    app_cfg.runtime.build()?.block_on(run(args, config))
}

/// Tokio runtime entry point.
async fn run(args: Args, mut config: ServiceConfig<LocalConfig>) -> Result<(), anyhow::Error> {
    // Initialize uxum handle, including logging and tracing.
    //
    // Logging will start working right after this call, and until the returned
    // guard is dropped.
    let mut handle = config.app.handle()?;
    // Create app builder from app config.
    //
    // Also enable the auth subsystem.
    let mut app_builder = AppBuilder::from_config(&config.app).with_basic_auth();
    // Some hard-coded parameters for built-in API documentation.
    app_builder.configure_api_doc(|api_doc| {
        api_doc
            .with_app_title("Redis KV")
            .with_description("example Redis KV getter/setter API service.")
            .with_contact_name("Uxum developers")
            .with_contact_url("http://uxum.example.com")
            .with_contact_email("example@example.com")
            .with_tag("get", Some("KV getters"), None::<&str>)
            .with_tag("set", Some("KV setters"), None::<&str>)
    });
    // Initialize application state.
    // FIXME: transparently initialize metrics and traces in correct order.
    let _ = app_builder.metrics();
    let manager = RedisConnectionManager::new(config.service.redis.url)?;
    let pool = bb8::Pool::builder()
        .max_size(config.service.redis.max_size)
        .min_idle(config.service.redis.min_idle)
        .idle_timeout(config.service.redis.idle_timeout)
        .build(manager)
        .await?;
    let pool = InstrumentedPool::instrument(Some("redis"), pool)?;
    let state = AppState(Arc::new(AppStateInner { args, pool }));
    app_builder.with_state(state);
    // Build main application router.
    let app = app_builder.build()?;
    // Convert into service.
    let svc = app.into_make_service_with_connect_info::<SocketAddr>();
    // Start the service.
    handle
        .run(config.server, svc, Some(Duration::from_secs(5)))
        .await
        .map_err(Into::into)
}

/// Request body for get method.
#[derive(Deserialize, JsonSchema)]
pub struct GetRequest {
    /// Key to read from storage.
    key: String,
}

/// Response body for get method.
#[derive(JsonSchema, Serialize)]
pub struct GetResponse {
    /// Key that was read from storage.
    key: String,
    /// Value that was read from storage.
    value: Option<String>,
}

/// Get value by key.
#[handler(tags = ["get"], permissions = ["get"])]
async fn get(
    state: State<AppState>,
    Json(req): Json<GetRequest>,
) -> Result<Json<GetResponse>, ApiError> {
    let mut rconn = state
        .pool
        .get()
        .await
        .map_err(|e| ApiError::Redis(e.to_string()))?;
    let resp = rconn
        .wrap_async_mut("get", |rconn| rconn.get(&req.key))
        .await
        .map_err(|e| ApiError::Redis(e.to_string()))?;
    Ok(Json(GetResponse {
        key: req.key,
        value: resp,
    }))
}

/// Request body for get method.
#[derive(Deserialize, JsonSchema)]
pub struct SetRequest {
    /// Key to write to storage.
    key: String,
    /// Value to write to storage.
    value: Option<String>,
}

/// Set value for specific key.
#[handler(tags = ["set"], permissions = ["set"])]
async fn set(state: State<AppState>, Json(req): Json<SetRequest>) -> Result<(), ApiError> {
    let mut rconn = state
        .pool
        .get()
        .await
        .map_err(|e| ApiError::Redis(e.to_string()))?;
    rconn
        .wrap_async_mut("set", |rconn| rconn.set::<_, _, ()>(req.key, req.value))
        .await
        .map_err(|e| ApiError::Redis(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_args() {
        use clap::CommandFactory;
        Args::command().debug_assert();
    }
}
