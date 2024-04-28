use std::{net::SocketAddr, time::Duration};

use config::{Config, File};
use serde::{Deserialize, Serialize};
use uxum::{prelude::*, GetResponseSchemas, ResponseSchema};

/// Root container for app configuration
#[derive(Deserialize)]
struct ServiceConfig {
    #[serde(flatten)]
    app: AppConfig,
    server: ServerBuilder,
}

/// Application entry point
#[tokio::main]
async fn main() {
    let mut config: ServiceConfig = Config::builder()
        .add_source(File::with_name("examples/advanced_server/config.yaml"))
        .build()
        .expect("Unable to load configuration")
        .try_deserialize()
        .expect("Error deserializing configuration");
    let _tele_guard = config
        .app
        .with_app_name("advanced_server")
        .with_app_version("1.2.3")
        .init_telemetry()
        .expect("Error initializing telemetry");
    let mut app_builder = AppBuilder::from_config(&config.app).with_basic_auth();
    app_builder.configure_api_doc(|api_doc| {
        api_doc
            .with_app_title("Advanced Server")
            .with_description("Kitchen sink primer for *various* library features.")
            .with_contact_name("Uxum developers")
            .with_contact_url("http://uxum.example.com")
            .with_contact_email("example@example.com")
            .with_tag("tag1", Some("Some tag"), Some("http://example.com/tag1"))
            .with_tag("tag2", Some("Some other tag"), None::<&str>)
    });
    let app = app_builder.build().expect("Unable to build app");
    config
        .server
        .build()
        .await
        .expect("Unable to build server")
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .expect("Server error");
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

#[handler]
async fn no_op() {}

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

/// Greet someone using a name from a text body
#[handler]
async fn name_from_text_body(body: String) -> String {
    format!("Hello {}!", body)
}

/// Greet someone using a name from a binary body
#[handler]
async fn name_from_binary_body(body: bytes::Bytes) -> String {
    format!("Hello {:?}!", body)
}

/// Greet someone using a name from a URL path element
#[handler(
    path = "/hello/:name",
    spec(
        docs(description = "Another link", url = "http://example.com/hello_name"),
        path_params(name(description = "Name to greet", allow_empty = true))
    )
)]
async fn name_from_path(args: Path<String>) -> String {
    format!("Hello {}!", args.0)
}

/// Requested operation
#[derive(Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ComputeOp {
    /// Add two arguments
    #[default]
    Add,
    /// Subtract second argument from the first
    Subtract,
    /// Multiply two arguments
    Multiply,
    /// Divide first argument by the second
    Divide,
}

/// Request body
#[derive(Deserialize, JsonSchema)]
pub struct ComputeRequest {
    /// First argument
    arg1: i64,
    /// Second argument
    arg2: i64,
    #[serde(default)]
    op: ComputeOp,
}

/// Result of computation
#[derive(JsonSchema, Serialize)]
pub struct ComputeResponse {
    result: i64,
}

/// Perform simple arithmetic
///
/// Gets an operator and two operands as input. Returns result of operation.
/// This is an example of using automatically (de)serialized JSON as
/// input and output of a method.
#[handler(method = "POST", spec(tag = "calc"))]
async fn compute(req: Json<ComputeRequest>) -> Json<ComputeResponse> {
    let result = match req.op {
        ComputeOp::Add => req.arg1 + req.arg2,
        ComputeOp::Subtract => req.arg1 - req.arg2,
        ComputeOp::Multiply => req.arg1 * req.arg2,
        ComputeOp::Divide => req.arg1 / req.arg2,
    };
    Json(ComputeResponse { result })
}

/// Return error sometimes
///
/// This is an example of returning Result from a handler.
///
/// Be aware that standard [`axum::IntoResponse`] implementation is used
/// here, which means error responses do not automatically get 4xx or 5xx
/// HTTP statuses.
///
/// For proper error response generation, see [`get_random_number`].
#[handler]
async fn maybe_error_strings() -> Result<String, String> {
    if rand::random() {
        Ok("No error.".into())
    } else {
        Err("Error!".into())
    }
}

/// Request body used in [`get_random_number`]
#[derive(Deserialize, JsonSchema)]
struct GetRandomRequest {
    /// Low bound for a value
    min: i64,
    /// High bound for a value
    max: i64,
}

/// Response body used in [`get_random_number`]
#[derive(JsonSchema, Serialize)]
struct GetRandomResponse {
    /// Generated random value
    value: i64,
}

/// Error message used in [`get_random_number`]
#[derive(Debug, JsonSchema, Serialize, thiserror::Error)]
enum GetRandomError {
    /// Generated number is too small
    #[error("Generated number is too small")]
    NumberTooSmall,
    /// Generated number is too large
    #[error("Generated number is too large")]
    NumberTooLarge,
}

impl IntoResponse for GetRandomError {
    fn into_response(self) -> axum::response::Response {
        // HTTP status code, headers and error body can be customized here.
        let mut resp = Json::from(self).into_response();
        *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        resp.headers_mut().insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static(mime::APPLICATION_JSON.as_ref()),
        );
        resp
    }
}

impl GetResponseSchemas for GetRandomError {
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

/// Return random number within supplied bounds
///
/// This is an example of using a custom error type in a handler.
#[handler]
async fn get_random_number(
    req: Json<GetRandomRequest>,
) -> Result<Json<GetRandomResponse>, GetRandomError> {
    let value = rand::random();
    if value < req.min {
        Err(GetRandomError::NumberTooSmall)
    } else if value > req.max {
        Err(GetRandomError::NumberTooLarge)
    } else {
        Ok(Json(GetRandomResponse { value }))
    }
}
