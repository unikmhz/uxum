//! Subsystem to gather and export application metrics.

use std::{
    borrow::Cow,
    collections::HashMap,
    future::Future,
    ops::Deref,
    pin::Pin,
    sync::Arc,
    task::{ready, Context, Poll},
    time::Instant,
};

use axum::{
    body::HttpBody,
    extract::{MatchedPath, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{self, Router},
};
use hyper::{Method, Request};
use opentelemetry::{
    global,
    metrics::{Counter, Histogram, MeterProvider, ObservableGauge, Unit, UpDownCounter},
    KeyValue,
};
use opentelemetry_sdk::{
    metrics::{new_view, Aggregation, Instrument, MeterProviderBuilder, Stream},
    Resource,
};
use pin_project::pin_project;
use prometheus::{Encoder, Registry, TextEncoder};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tower::{Layer, Service};
use tracing::{debug_span, trace};

use crate::layers::ext::HandlerName;

/// Error type used in metrics subsystem.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MetricsError {
    /// Prometheus exporter error.
    #[error("Prometheus error: {0}")]
    Prometheus(#[from] prometheus::Error),
    /// OpenTelemetry metrics error.
    #[error("OTel metrics error: {0}")]
    OpenTelemetry(#[from] opentelemetry::metrics::MetricsError),
}

impl IntoResponse for MetricsError {
    fn into_response(self) -> Response {
        problemdetails::new(StatusCode::INTERNAL_SERVER_ERROR)
            .with_title(self.to_string())
            .into_response()
    }
}

/// Configuration and builder for metrics subsystem.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct MetricsBuilder {
    /// Whether HTTP metrics gathering is enabled.
    #[serde(default = "crate::util::default_true")]
    enabled: bool,
    /// Histogram metric buckets for total request duration.
    ///
    /// Measured in seconds.
    #[serde(default = "MetricsBuilder::default_duration_buckets")]
    duration_buckets: Vec<f64>,
    /// Histogram metric buckets for request size.
    ///
    /// Measured in bytes.
    #[serde(default = "MetricsBuilder::default_size_buckets")]
    size_buckets: Vec<f64>,
    /// URL path for metrics prometheus exporter endpoint.
    #[serde(default = "MetricsBuilder::default_metrics_path")]
    metrics_path: String,
    /// Static labels to add to gathered metrics.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    labels: HashMap<String, String>,
    /// Optional prefix for metric names.
    #[serde(default)]
    prefix: Option<String>,
}

impl Default for MetricsBuilder {
    fn default() -> Self {
        Self {
            enabled: true,
            duration_buckets: Self::default_duration_buckets(),
            size_buckets: Self::default_size_buckets(),
            metrics_path: Self::default_metrics_path(),
            labels: HashMap::new(),
            prefix: None,
        }
    }
}

impl MetricsBuilder {
    /// Default value for [`Self::duration_buckets`].
    #[must_use]
    #[inline]
    fn default_duration_buckets() -> Vec<f64> {
        [
            0.0_f64, 0.0025_f64, 0.005_f64, 0.01_f64, 0.025_f64, 0.05_f64, 0.075_f64, 0.1_f64,
            0.25_f64, 0.5_f64, 0.75_f64, 1.0_f64, 2.5_f64, 5.0_f64, 7.5_f64, 10.0_f64, 30.0_f64,
        ]
        .into()
    }

    /// Default value for [`Self::size_buckets`].
    #[must_use]
    #[inline]
    fn default_size_buckets() -> Vec<f64> {
        const KB: f64 = 1024.0;
        const MB: f64 = 1024.0 * KB;

        [
            0.0_f64,
            64.0_f64,
            128.0_f64,
            256.0_f64,
            512.0_f64,
            1.0_f64 * KB,
            2.0_f64 * KB,
            4.0_f64 * KB,
            8.0_f64 * KB,
            16.0_f64 * KB,
            32.0_f64 * KB,
            64.0_f64 * KB,
            128.0_f64 * KB,
            256.0_f64 * KB,
            512.0_f64 * KB,
            1.0_f64 * MB,
            2.0_f64 * MB,
            4.0_f64 * MB,
            8.0_f64 * MB,
        ]
        .into()
    }

    /// Default value for [`Self::metrics_path`].
    #[must_use]
    #[inline]
    fn default_metrics_path() -> String {
        "/metrics".into()
    }

    /// Whether HTTP metrics gathering is enabled.
    #[must_use]
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Set histogram metric buckets for total request duration.
    #[must_use]
    pub fn with_duration_buckets<B, I>(mut self, buckets: B) -> Self
    where
        B: IntoIterator<Item = I>,
        I: Into<f64>,
    {
        self.duration_buckets = buckets.into_iter().map(Into::into).collect();
        self
    }

    /// Set histogram metric buckets for request size.
    #[must_use]
    pub fn with_size_buckets<B, I>(mut self, buckets: B) -> Self
    where
        B: IntoIterator<Item = I>,
        I: Into<f64>,
    {
        self.size_buckets = buckets.into_iter().map(Into::into).collect();
        self
    }

    /// Set URL path for metrics prometheus exporter endpoint.
    #[must_use]
    pub fn with_metrics_path<B, S>(mut self, path: impl ToString) -> Self {
        self.metrics_path = path.to_string();
        self
    }

    /// Add one static label to be added to gathered metrics.
    #[must_use]
    pub fn with_label<T, U>(mut self, key: T, value: U) -> Self
    where
        T: ToString,
        U: ToString,
    {
        self.labels.insert(key.to_string(), value.to_string());
        self
    }

    /// Add multiple static labels to be added to gathered metrics.
    #[must_use]
    pub fn with_labels<'a, T, U, V>(mut self, kvs: V) -> Self
    where
        T: ToString + 'a,
        U: ToString + 'a,
        V: IntoIterator<Item = (&'a T, &'a U)>,
    {
        self.labels.extend(
            kvs.into_iter()
                .map(|(key, val)| (key.to_string(), val.to_string())),
        );
        self
    }

    /// Set optional prefix for metric names.
    ///
    /// Defaults to no prefix used.
    #[must_use]
    pub fn with_prefix(mut self, prefix: impl ToString) -> Self {
        self.prefix = Some(prefix.to_string());
        self
    }

    /// Build new Prometheus registry.
    fn build_prometheus_registry(&self) -> Result<Registry, MetricsError> {
        Registry::new_custom(
            self.prefix.clone(),
            if self.labels.is_empty() {
                None
            } else {
                Some(self.labels.clone())
            },
        )
        .map_err(Into::into)
    }

    /// Build metrics state object.
    ///
    /// # Errors
    ///
    /// Returns `Err` if metrics registry or provider could not be initialized.
    pub fn build_state(&self, resource: Resource) -> Result<MetricsState, MetricsError> {
        let _span = debug_span!("build_metrics").entered();
        let registry = self.build_prometheus_registry()?;
        let exporter = opentelemetry_prometheus::exporter()
            .with_registry(registry.clone())
            .build()?;
        let provider = MeterProviderBuilder::default()
            .with_resource(resource)
            .with_reader(exporter)
            .with_view(new_view(
                Instrument::new().name("*http.server.request.duration"),
                Stream::new().aggregation(Aggregation::ExplicitBucketHistogram {
                    boundaries: self.duration_buckets.clone(),
                    record_min_max: true,
                }),
            )?)
            .with_view(new_view(
                Instrument::new().name("*http.server.request.body.size"),
                Stream::new().aggregation(Aggregation::ExplicitBucketHistogram {
                    boundaries: self.size_buckets.clone(),
                    record_min_max: true,
                }),
            )?)
            .with_view(new_view(
                Instrument::new().name("*http.server.response.body.size"),
                Stream::new().aggregation(Aggregation::ExplicitBucketHistogram {
                    boundaries: self.size_buckets.clone(),
                    record_min_max: true,
                }),
            )?)
            .build();

        global::set_meter_provider(provider.clone());
        let meter = provider.meter("axum-app");

        // TODO: try_init() and handle errors.

        // HTTP server metrics.
        let request_duration = meter
            .f64_histogram("http.server.request.duration")
            .with_unit(Unit::new("s"))
            .with_description("The HTTP request latencies in seconds.")
            .init();
        let requests_total = meter
            .u64_counter("http.server.requests")
            .with_description(
                "How many HTTP requests processed, partitioned by status code and HTTP method.",
            )
            .init();
        let requests_active = meter
            .i64_up_down_counter("http.server.active_requests")
            .with_description("The number of active HTTP requests.")
            .init();
        let request_body_size = meter
            .u64_histogram("http.server.request.body.size")
            .with_unit(Unit::new("By"))
            .with_description("The HTTP request body sizes in bytes.")
            .init();
        let response_body_size = meter
            .u64_histogram("http.server.response.body.size")
            .with_unit(Unit::new("By"))
            .with_description("The HTTP reponse body sizes in bytes.")
            .init();
        let http_server = HttpServerMetrics {
            request_duration,
            requests_total,
            requests_active,
            request_body_size,
            response_body_size,
        };

        // HTTP client metrics
        let request_duration = meter
            .f64_histogram("http.client.request.duration")
            .with_unit(Unit::new("s"))
            .with_description("The HTTP request latencies in seconds.")
            .init();
        let requests_total = meter
            .u64_counter("http.client.requests")
            .with_description(
                "How many HTTP requests processed, partitioned by status code and HTTP method.",
            )
            .init();
        let requests_active = meter
            .i64_up_down_counter("http.client.active_requests")
            .with_description("The number of active HTTP requests.")
            .init();
        let request_body_size = meter
            .u64_histogram("http.client.request.body.size")
            .with_unit(Unit::new("By"))
            .with_description("The HTTP request body sizes in bytes.")
            .init();
        let response_body_size = meter
            .u64_histogram("http.client.response.body.size")
            .with_unit(Unit::new("By"))
            .with_description("The HTTP reponse body sizes in bytes.")
            .init();
        let http_client = HttpClientMetricsInner {
            request_duration,
            requests_total,
            requests_active,
            request_body_size,
            response_body_size,
        };
        let http_client = HttpClientMetrics(Arc::new(http_client));

        // Tokio runtime metrics.
        let num_workers = meter
            .u64_observable_gauge("runtime.workers")
            .with_description("Number of worker threads used by the runtime.")
            .init();
        let num_alive_tasks = meter
            .u64_observable_gauge("runtime.alive_tasks")
            .with_description("Current number of alive tasks in the runtime.")
            .init();
        let runtime = RuntimeMetrics {
            num_workers,
            num_alive_tasks,
        };

        Ok(MetricsState {
            registry,
            http_server,
            http_client,
            runtime,
            metrics_path: self.metrics_path.clone(),
        })
    }
}

/// Metrics state [`tower`] layer.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct MetricsState {
    /// Prometheus registry.
    ///
    /// Holds all configured metrics and their collected values.
    registry: Registry,
    /// HTTP server metrics.
    http_server: HttpServerMetrics,
    /// HTTP client metrics.
    http_client: HttpClientMetrics,
    /// Tokio runtime metrics.
    runtime: RuntimeMetrics,
    /// URL path for metrics prometheus exporter.
    metrics_path: String,
}

/// Container for HTTP server metrics.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub(crate) struct HttpServerMetrics {
    /// Distribution of request handling durations.
    request_duration: Histogram<f64>,
    /// Lifetime counter of received requests.
    requests_total: Counter<u64>,
    /// Currently active requests.
    requests_active: UpDownCounter<i64>,
    /// Distribution of request body sizes.
    request_body_size: Histogram<u64>,
    /// Distribution of response body sizes.
    response_body_size: Histogram<u64>,
}

/// Shared container for HTTP client metrics
#[derive(Clone, Debug)]
#[repr(transparent)]
pub struct HttpClientMetrics(Arc<HttpClientMetricsInner>);

impl Deref for HttpClientMetrics {
    type Target = HttpClientMetricsInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Inner container for HTTP client metrics.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct HttpClientMetricsInner {
    /// Distribution of request handling durations.
    pub request_duration: Histogram<f64>,
    /// Lifetime counter of received requests.
    pub requests_total: Counter<u64>,
    /// Currently active requests.
    pub requests_active: UpDownCounter<i64>,
    /// Distribution of request body sizes.
    pub request_body_size: Histogram<u64>,
    /// Distribution of response body sizes.
    pub response_body_size: Histogram<u64>,
}

/// Container for Tokio runtime metrics.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub(crate) struct RuntimeMetrics {
    /// Number of worker threads used by the runtime.
    ///
    /// The number of workers is set by configuring `worker_threads` on [`tokio::runtime::Builder`].
    /// When using the `current_thread` runtime, the return value is always `1`.
    num_workers: ObservableGauge<u64>,
    /// Current number of alive tasks in the runtime.
    ///
    /// This counter increases when a task is spawned and decreases when a task exits.
    num_alive_tasks: ObservableGauge<u64>,
}

impl<S> Layer<S> for MetricsState {
    type Service = HttpMetrics<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HttpMetrics {
            state: self.clone(),
            inner,
        }
    }
}

impl MetricsState {
    /// Build Axum router containing all metrics methods.
    pub fn build_router(&self) -> Router {
        let _span = debug_span!("build_metrics_router").entered();
        Router::new()
            .route(&self.metrics_path, routing::get(get_metrics))
            .with_state(self.clone())
    }

    /// Get HTTP client metrics state object.
    #[must_use]
    pub fn client_metrics(&self, name: impl AsRef<str>) -> ClientMetricsState {
        ClientMetricsState {
            name: name.as_ref().to_string(),
            metrics: self.http_client.clone(),
        }
    }
}

/// HTTP client metrics state object.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ClientMetricsState {
    /// Client name label.
    name: String,
    /// Metrics container.
    metrics: HttpClientMetrics,
}

impl ClientMetricsState {
    /// Get client name label.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get metrics container.
    #[must_use]
    pub fn metrics(&self) -> &HttpClientMetrics {
        &self.metrics
    }
}

/// Metrics state [`tower`] service.
#[derive(Clone)]
#[non_exhaustive]
pub struct HttpMetrics<S> {
    /// Shared state for all metered requests.
    state: MetricsState,
    /// Inner service.
    inner: S,
}

impl<S, T, U> Service<Request<T>> for HttpMetrics<S>
where
    S: Service<Request<T>, Response = Response<U>>,
    T: HttpBody,
    U: HttpBody,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = HttpMetricsFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<T>) -> Self::Future {
        let start = Instant::now();
        let ext = req.extensions();
        let method = req.method().clone();
        let scheme = match req.uri().scheme() {
            Some(sch) => sch.to_string(),
            None => String::new(),
        };
        let path = ext.get::<MatchedPath>().cloned();
        let request_size = req.size_hint().upper().unwrap_or_default();
        self.state.http_server.requests_active.add(
            1,
            &[
                KeyValue::new("http.request.method", method.to_string()),
                KeyValue::new("url.scheme", scheme.clone()),
            ],
        );
        HttpMetricsFuture {
            inner: self.inner.call(req),
            state: self.state.clone(),
            start,
            method,
            scheme,
            path,
            request_size,
        }
    }
}

/// Response future for [`HttpMetrics`] middleware
#[pin_project]
#[non_exhaustive]
pub struct HttpMetricsFuture<F> {
    /// Inner future.
    #[pin]
    inner: F,
    /// Shared state for all metered requests.
    state: MetricsState,
    /// Request processing beginning timestamp.
    start: Instant,
    /// HTTP request method.
    method: Method,
    /// HTTP URI scheme.
    scheme: String,
    /// Matched [`axum`] route.
    path: Option<MatchedPath>,
    /// HTTP request size, in bytes.
    request_size: u64,
}

impl<F, U, E> Future for HttpMetricsFuture<F>
where
    F: Future<Output = Result<Response<U>, E>>,
    U: HttpBody,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let resp_result = ready!(this.inner.poll(cx));

        let kv_method = KeyValue::new("http.request.method", this.method.to_string());
        let kv_scheme = KeyValue::new("url.scheme", this.scheme.clone());
        this.state
            .http_server
            .requests_active
            .add(-1, &[kv_method.clone(), kv_scheme.clone()]);

        let resp = resp_result?;
        let handler = resp.extensions().get::<HandlerName>();
        let duration = this.start.elapsed().as_secs_f64();
        let status = resp.status().as_str().to_owned();
        let response_size = resp.size_hint().upper().unwrap_or(0);

        let labels = [
            kv_method,
            kv_scheme,
            KeyValue::new("http.response.status_code", status),
            KeyValue::new(
                "http.route",
                this.path.as_ref().map_or(Cow::Borrowed(""), |path| {
                    Cow::Owned(path.as_str().to_owned())
                }),
            ),
            KeyValue::new("uxum.handler", handler.map_or("", |hdl| hdl.as_str())),
        ];
        // server.address?
        // server.port?
        // network.protocol.name?
        // network.protocol.version?
        this.state.http_server.requests_total.add(1, &labels);
        this.state
            .http_server
            .request_duration
            .record(duration, &labels);
        this.state
            .http_server
            .request_body_size
            .record(*this.request_size, &labels);
        this.state
            .http_server
            .response_body_size
            .record(response_size, &labels);
        trace!("metrics recorded");

        Poll::Ready(Ok(resp))
    }
}

/// Method handler to generate metrics
async fn get_metrics(metrics: State<MetricsState>) -> Result<impl IntoResponse, MetricsError> {
    // Record runtime metrics just-in-time
    let rt_metrics = tokio::runtime::Handle::current().metrics();
    metrics
        .runtime
        .num_workers
        .observe(rt_metrics.num_workers() as u64, &[]);
    metrics
        .runtime
        .num_alive_tasks
        .observe(rt_metrics.num_alive_tasks() as u64, &[]);

    // Serialize metrics
    let encoder = TextEncoder::new();
    let mut buf = Vec::new();
    encoder.encode(&metrics.registry.gather(), &mut buf)?;
    Ok(([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], buf))
}
