use std::{net::SocketAddr, time::Duration};

use serde::{Deserialize, Serialize};
use uxum::{prelude::*, GetResponseSchemas, ResponseSchema};

/// Application entry point.
fn main() -> Result<(), HandleError> {
    // Load configuration from file.
    let mut config = ServiceConfig::builder()
        .with_file("examples/advanced_server/config.yaml")
        .build()
        .expect("Unable to load configuration");
    // Add some hard-coded values to [`AppConfig`].
    let app_cfg = config
        .app
        .with_app_name("advanced_server")
        .with_app_version("1.2.3");
    // Build and start Tokio runtime.
    app_cfg
        .runtime
        .build()
        .expect("Error creating Tokio runtime")
        .block_on(run(config))
}

/// Tokio runtime entry point.
async fn run(mut config: ServiceConfig) -> Result<(), HandleError> {
    // Initialize uxum handle, including logging and tracing.
    //
    // Logging will start working right after this call, and until the returned
    // guard is dropped.
    let mut handle = config.app.handle().expect("Error initializing handle");
    // Create app builder from app config.
    //
    // Also enable the auth subsystem.
    let mut app_builder = AppBuilder::from_config(&config.app).with_basic_auth();
    // Some hard-coded parameters for built-in API documentation.
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
    // Initialize required states.
    let tracing_client = app_builder
        .http_client_or_default("tracing")
        .await
        .expect("No tracing HTTP client");
    app_builder
        .with_state(distributed_tracing::TracingState::from(tracing_client))
        .with_state(counter_state::CounterState::default())
        .with_state(hello::HelloState::new());
    // Build main application router.
    let app = app_builder.build().expect("Unable to build app");
    // Start the service.
    handle
        .run(config.server, app, Some(Duration::from_secs(5)))
        .await
}

/// Sleep for some time and return response.
#[handler]
async fn sleep(ConnectInfo(client): ConnectInfo<SocketAddr>) -> String {
    tokio::time::sleep(Duration::from_secs(3)).await;
    format!("Hello {client}! Woken up after 3 seconds!")
}

/// Panic within the handler.
#[handler]
async fn panic() {
    panic!("NOOOOOOOO!");
}

/// Authentication is disabled for this handler.
#[handler(no_auth)]
async fn no_op() {}

/// Requested operation.
#[derive(Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ComputeOp {
    /// Add two arguments.
    #[default]
    Add,
    /// Subtract second argument from the first.
    Subtract,
    /// Multiply two arguments.
    Multiply,
    /// Divide first argument by the second.
    Divide,
}

/// Request body.
#[derive(Deserialize, JsonSchema)]
pub struct ComputeRequest {
    /// First argument.
    arg1: i64,
    /// Second argument.
    arg2: i64,
    #[serde(default)]
    op: ComputeOp,
}

/// Result of computation.
#[derive(JsonSchema, Serialize)]
pub struct ComputeResponse {
    /// Computed value.
    result: i64,
}

/// Perform simple arithmetic.
///
/// Gets an operator and two operands as input. Returns result of operation.
/// This is an example of using automatically (de)serialized JSON as
/// input and output of a method.
#[handler(method = "POST", tags = ["calc"])]
async fn compute(req: Json<ComputeRequest>) -> Json<ComputeResponse> {
    let result = match req.op {
        ComputeOp::Add => req.arg1 + req.arg2,
        ComputeOp::Subtract => req.arg1 - req.arg2,
        ComputeOp::Multiply => req.arg1 * req.arg2,
        ComputeOp::Divide => req.arg1 / req.arg2,
    };
    Json(ComputeResponse { result })
}

/// Return error sometimes.
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

/// Request body used in [`get_random_number`].
#[derive(Deserialize, JsonSchema)]
struct GetRandomRequest {
    /// Low bound for a value.
    min: i64,
    /// High bound for a value.
    max: i64,
}

/// Response body used in [`get_random_number`].
#[derive(JsonSchema, Serialize)]
struct GetRandomResponse {
    /// Generated random value.
    value: i64,
}

/// Error message used in [`get_random_number`].
#[derive(Debug, JsonSchema, Serialize, thiserror::Error)]
enum GetRandomError {
    /// Generated number is too small.
    #[error("Generated number is too small")]
    NumberTooSmall,
    /// Generated number is too large.
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

/// Return random number within supplied bounds.
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

/// Example of using a shared app state object.
mod counter_state {
    use std::{
        ops::Deref,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };

    use super::*;

    /// App state used in [`inc_state`] and [`dec_state`] handlers.
    #[derive(Clone)]
    pub struct CounterState(Arc<CounterStateInner>);

    impl Deref for CounterState {
        type Target = CounterStateInner;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    /// Internal shared struct.
    pub struct CounterStateInner {
        /// Stored atomic counter value.
        counter: AtomicUsize,
    }

    impl Default for CounterState {
        fn default() -> Self {
            Self(Arc::new(CounterStateInner {
                counter: AtomicUsize::new(0),
            }))
        }
    }

    /// State methods.
    impl CounterState {
        /// Add 1 to counter.
        pub fn inc(&self) -> usize {
            self.counter.fetch_add(1, Ordering::Relaxed)
        }

        /// Subtract 1 from counter.
        pub fn dec(&self) -> usize {
            self.counter.fetch_sub(1, Ordering::Relaxed)
        }
    }

    /// Increase persistent counter.
    #[handler(tags = ["counter"])]
    async fn inc_state(state: State<CounterState>) -> String {
        let old = state.inc();
        format!("Old counter value was {old}")
    }

    /// Decrease persistent counter.
    #[handler(tags = ["counter"])]
    async fn dec_state(state: State<CounterState>) -> String {
        let old = state.dec();
        format!("Old counter value was {old}")
    }
}

/// Sample greeting methods with a custom metric.
mod hello {
    use bytes::Bytes;
    use opentelemetry::{global, metrics::Counter, KeyValue};

    use super::*;

    /// App state used in hello handlers.
    #[derive(Clone)]
    pub struct HelloState {
        /// Metric: number of times each name was greeted.
        num_greetings: Counter<u64>,
    }

    impl HelloState {
        /// Create new instance of hello handlers app state.
        pub fn new() -> Self {
            let meter = global::meter("hello");
            let num_greetings = meter
                .u64_counter("num_greetings")
                .with_description("Number of times each name was greeted.")
                .init();
            HelloState { num_greetings }
        }

        pub fn log_name(&self, name: impl AsRef<str>) {
            self.num_greetings
                .add(1, &[KeyValue::new("name", name.as_ref().to_string())]);
        }
    }

    /// Greet the Axum world.
    #[handler(
        name = "hello_world",
        path = "/",
        method = "GET",
        docs(description = "Some link", url = "http://example.com/hello_world"),
        tags = ["tag1", "tag2"],
        permissions = ["perm1"]
    )]
    async fn root_handler(state: State<HelloState>) -> &'static str {
        state.log_name("");
        tracing::info!("Said hello to the Axum world");
        "Hello Axum world!"
    }

    /// Query parameters.
    #[derive(Deserialize, JsonSchema)]
    struct QueryName {
        /// Name of the person to greet.
        #[serde(default = "QueryName::default_name")]
        name: String,
    }

    impl QueryName {
        fn default_name() -> String {
            "Jebediah".into()
        }
    }

    /// Greet someone using a name from a query string.
    #[handler]
    async fn name_from_qs(state: State<HelloState>, q: Query<QueryName>) -> String {
        state.log_name(&q.name);
        format!("Hello {}!", q.name)
    }

    /// Greet someone using a name from a text body.
    #[handler]
    async fn name_from_text_body(state: State<HelloState>, body: String) -> String {
        state.log_name(&body);
        format!("Hello {}!", body)
    }

    /// Greet someone using a name from a binary body.
    #[handler]
    async fn name_from_binary_body(state: State<HelloState>, body: Bytes) -> String {
        state.log_name(std::str::from_utf8(&body).unwrap_or(""));
        format!("Hello {:?}!", body)
    }

    /// Greet someone using a name from a URL path element.
    #[handler(
        path = "/hello/:name",
        docs(description = "Another link", url = "http://example.com/hello_name"),
        path_params(name(description = "Name to greet", allow_empty = true))
    )]
    async fn name_from_path(state: State<HelloState>, args: Path<String>) -> String {
        state.log_name(&args.0);
        format!("Hello {}!", args.0)
    }
}

/// This is used to test distributed tracing.
mod distributed_tracing {
    use std::{ops::Deref, sync::Arc};

    use super::*;

    /// Error message used in [`call_inner`].
    #[derive(Debug, thiserror::Error)]
    enum CallInnerError {
        /// Error produced by [`reqwest`] crate.
        #[error(transparent)]
        Reqwest(#[from] reqwest::Error),
        /// Error produced by [`reqwest_middleware`] crate.
        #[error(transparent)]
        ReqwestMiddleware(#[from] reqwest_middleware::Error),
    }

    impl IntoResponse for CallInnerError {
        fn into_response(self) -> axum::response::Response {
            problemdetails::new(StatusCode::INTERNAL_SERVER_ERROR)
                .with_title(self.to_string())
                .into_response()
        }
    }

    // I didn't bother to provide correct schema here.
    impl GetResponseSchemas for CallInnerError {
        type ResponseIter = [ResponseSchema; 1];
        fn get_response_schemas(_gen: &mut schemars::gen::SchemaGenerator) -> Self::ResponseIter {
            [ResponseSchema {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                response: openapi3::Response {
                    description: "Error response".into(),
                    content: okapi::map! {
                        mime::APPLICATION_JSON.to_string() => openapi3::MediaType {
                            ..Default::default()
                        },
                    },
                    ..Default::default()
                },
            }]
        }
    }

    /// App state used in [`call_inner`] handler.
    #[derive(Clone)]
    pub struct TracingState(Arc<TracingStateInner>);

    impl Deref for TracingState {
        type Target = TracingStateInner;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl From<reqwest_middleware::ClientWithMiddleware> for TracingState {
        fn from(client: reqwest_middleware::ClientWithMiddleware) -> Self {
            Self(Arc::new(TracingStateInner { client }))
        }
    }

    /// Internal shared struct.
    pub struct TracingStateInner {
        client: reqwest_middleware::ClientWithMiddleware,
    }

    /// Call inner service and return its response.
    #[handler]
    async fn call_inner(state: State<TracingState>) -> Result<String, CallInnerError> {
        Ok(state
            .client
            .get("http://127.0.0.1:8081/inner")
            .send()
            .await?
            .text()
            .await?)
    }
}
