use std::{
    borrow::Cow,
    collections::HashMap,
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll},
    time::{Duration, Instant},
};

use axum::{
    extract::{MatchedPath, State},
    response::IntoResponse,
    routing::{self, Router},
};
use hyper::{body::HttpBody, Method, Request, Response};
use opentelemetry::{
    global,
    metrics::{Counter, Histogram, MeterProvider as _, Unit, UpDownCounter},
    KeyValue,
};
use opentelemetry_sdk::{
    metrics::{new_view, Aggregation, Instrument, MeterProvider, Stream},
    resource::{EnvResourceDetector, SdkProvidedResourceDetector, TelemetryResourceDetector},
    Resource,
};
use opentelemetry_semantic_conventions::resource as res;
use pin_project::pin_project;
use prometheus::{Encoder, Registry, TextEncoder};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tower::{Layer, Service};
use tracing::{debug_span, trace};

use crate::layers::ext::HandlerName;

/// Error type used in metrics subsystem
#[derive(Debug, Error)]
pub enum MetricsError {
    /// Prometheus exporter error
    #[error("Prometheus error: {0}")]
    Prometheus(#[from] prometheus::Error),
    /// OpenTelemetry metrics error
    #[error("OTel metrics error: {0}")]
    OpenTelemetry(#[from] opentelemetry::metrics::MetricsError),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MetricsBuilder {
    /// Whether HTTP metrics gathering is enabled
    #[serde(default = "crate::util::default_true")]
    enabled: bool,
    /// Histogram metric buckets for total request duration
    ///
    /// Measured in seconds.
    #[serde(default = "MetricsBuilder::default_duration_buckets")]
    duration_buckets: Vec<f64>,
    /// Histogram metric buckets for request size
    ///
    /// Measured in bytes.
    #[serde(default = "MetricsBuilder::default_size_buckets")]
    size_buckets: Vec<f64>,
    /// URL path for metrics prometheus exporter
    #[serde(default = "MetricsBuilder::default_metrics_path")]
    metrics_path: String,
    /// Static labels to add to gathered metrics
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    labels: HashMap<String, String>,
    /// Optional prefix to use before
    #[serde(default)]
    prefix: Option<String>,
    /// OpenTelemetry resource detection timeout
    #[serde(
        default = "MetricsBuilder::default_detector_timeout",
        with = "humantime_serde"
    )]
    detector_timeout: Duration,
    /// App namespace
    app_namespace: Option<String>,
    /// Short app name
    #[serde(default)]
    app_name: Option<String>,
    /// App version
    #[serde(default)]
    app_version: Option<String>,
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
            detector_timeout: Self::default_detector_timeout(),
            app_namespace: None,
            app_name: None,
            app_version: None,
        }
    }
}

impl MetricsBuilder {
    /// Default value for [`Self::duration_buckets`]
    #[must_use]
    fn default_duration_buckets() -> Vec<f64> {
        [
            0.0, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 7.5,
            10.0, 30.0,
        ]
        .into()
    }

    /// Default value for [`Self::size_buckets`]
    #[must_use]
    fn default_size_buckets() -> Vec<f64> {
        const KB: f64 = 1024.0;
        const MB: f64 = 1024.0 * KB;

        [
            0.0,
            64.0,
            128.0,
            256.0,
            512.0,
            1.0 * KB,
            2.0 * KB,
            4.0 * KB,
            8.0 * KB,
            16.0 * KB,
            32.0 * KB,
            64.0 * KB,
            128.0 * KB,
            256.0 * KB,
            512.0 * KB,
            1.0 * MB,
            2.0 * MB,
            4.0 * MB,
            8.0 * MB,
        ]
        .into()
    }

    /// Default value for [`Self::metrics_path`]
    #[must_use]
    fn default_metrics_path() -> String {
        "/metrics".into()
    }

    /// Default value for [`Self::detector_timeout`]
    fn default_detector_timeout() -> Duration {
        Duration::from_secs(6)
    }

    /// Whether HTTP metrics gathering is enabled
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Set histogram metric buckets for total request duration
    #[must_use]
    pub fn with_duration_buckets<B, I>(mut self, buckets: B) -> Self
    where
        B: IntoIterator<Item = I>,
        I: Into<f64>,
    {
        self.duration_buckets = buckets.into_iter().map(Into::into).collect();
        self
    }

    /// Set histogram metric buckets for request size
    #[must_use]
    pub fn with_size_buckets<B, I>(mut self, buckets: B) -> Self
    where
        B: IntoIterator<Item = I>,
        I: Into<f64>,
    {
        self.size_buckets = buckets.into_iter().map(Into::into).collect();
        self
    }

    /// Set URL path for metrics prometheus exporter
    #[must_use]
    pub fn with_metrics_path(mut self, path: impl ToString) -> Self {
        self.metrics_path = path.to_string();
        self
    }

    /// Add one static label to be added to gathered metrics
    #[must_use]
    pub fn with_label<T, U>(mut self, key: T, value: U) -> Self
    where
        T: ToString,
        U: ToString,
    {
        self.labels.insert(key.to_string(), value.to_string());
        self
    }

    /// Add multiple static labels to be added to gathered metrics
    #[must_use]
    pub fn with_labels<'a, T, U, V>(mut self, kvs: V) -> Self
    where
        T: ToString + 'a,
        U: ToString + 'a,
        V: IntoIterator<Item = (&'a T, &'a U)>,
    {
        self.labels
            .extend(kvs.into_iter().map(|(k, v)| (k.to_string(), v.to_string())));
        self
    }

    /// Set optional prefix to use before
    #[must_use]
    pub fn with_prefix(mut self, prefix: impl ToString) -> Self {
        self.prefix = Some(prefix.to_string());
        self
    }

    /// Set app namespace
    #[must_use]
    pub fn with_app_namespace(mut self, namespace: impl ToString) -> Self {
        self.app_namespace = Some(namespace.to_string());
        self
    }

    /// Set short app name
    #[must_use]
    pub fn with_app_name(mut self, name: impl ToString) -> Self {
        self.app_name = Some(name.to_string());
        self
    }

    /// Set app version
    #[must_use]
    pub fn with_app_version(mut self, version: impl ToString) -> Self {
        self.app_version = Some(version.to_string());
        self
    }

    /// Set fallback app name and version
    ///
    /// This gets called from [`crate::AppBuilder`]
    pub fn set_app_defaults(
        &mut self,
        name: Option<impl ToString>,
        version: Option<impl ToString>,
    ) {
        if self.app_name.is_none() {
            self.app_name = name.map(|s| s.to_string());
        }
        if self.app_version.is_none() {
            self.app_version = version.map(|s| s.to_string());
        }
    }

    /// Build new Prometheus registry
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

    /// Build metrics state object
    pub fn build_state(&self) -> Result<MetricsState, MetricsError> {
        let _span = debug_span!("build_metrics").entered();
        let registry = self.build_prometheus_registry()?;
        let mut resource = Resource::from_detectors(
            self.detector_timeout,
            vec![
                Box::new(SdkProvidedResourceDetector),
                Box::new(EnvResourceDetector::new()),
                Box::new(TelemetryResourceDetector),
            ],
        );
        let mut static_resources = Vec::new();
        if let Some(app_namespace) = &self.app_namespace {
            static_resources.push(res::SERVICE_NAMESPACE.string(app_namespace.clone()));
        }
        if let Some(app_name) = &self.app_name {
            static_resources.push(res::SERVICE_NAME.string(app_name.clone()));
        }
        if let Some(app_version) = &self.app_version {
            static_resources.push(res::SERVICE_VERSION.string(app_version.clone()));
        }
        if !static_resources.is_empty() {
            resource = resource.merge(&mut Resource::new(static_resources));
        }
        let exporter = opentelemetry_prometheus::exporter()
            .with_registry(registry.clone())
            .build()?;
        let provider = MeterProvider::builder()
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

        let request_duration = meter
            .f64_histogram("http.server.request.duration")
            .with_unit(Unit::new("s"))
            .with_description("The HTTP request latencies in seconds.")
            .init();
        let requests_total = meter
            .u64_counter("requests")
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

        Ok(MetricsState {
            registry,
            http_server: HttpServerMetrics {
                request_duration,
                requests_total,
                requests_active,
                request_body_size,
                response_body_size,
            },
            metrics_path: self.metrics_path.clone(),
        })
    }
}

/// Metrics state object
#[derive(Clone)]
pub struct MetricsState {
    /// Prometheus registry
    ///
    /// Holds all configured metrics and their collected values.
    registry: Registry,
    /// HTTP server metrics
    http_server: HttpServerMetrics,
    /// URL path for metrics prometheus exporter
    metrics_path: String,
}

/// Container for HTTP server metrics
#[derive(Clone)]
pub(crate) struct HttpServerMetrics {
    /// Distribution of request handling durations
    request_duration: Histogram<f64>,
    /// Lifetime counter of received requests
    requests_total: Counter<u64>,
    /// Currently active requests
    requests_active: UpDownCounter<i64>,
    /// Distribution of request body sizes
    request_body_size: Histogram<u64>,
    /// Distribution of response body sizes
    response_body_size: Histogram<u64>,
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
    /// Build Axum router containing all metrics methods
    pub fn build_router(&self) -> Router {
        let _span = debug_span!("build_metrics_router").entered();
        Router::new()
            .route(&self.metrics_path, routing::get(get_metrics))
            .with_state(self.clone())
    }
}

///
#[derive(Clone)]
pub struct HttpMetrics<S> {
    state: MetricsState,
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
        // FIXME: HandlerName is not found
        let handler = ext.get::<HandlerName>().copied();
        let path = ext.get::<MatchedPath>().cloned();
        let request_size = req.size_hint().upper().unwrap_or(0);
        // FIXME: get scheme from request
        self.state.http_server.requests_active.add(
            1,
            &[
                KeyValue::new("http.request.method", method.to_string()),
                KeyValue::new("url.scheme", "http"),
            ],
        );
        HttpMetricsFuture {
            inner: self.inner.call(req),
            state: self.state.clone(),
            start,
            method,
            handler,
            path,
            request_size,
        }
    }
}

///
#[pin_project]
pub struct HttpMetricsFuture<F> {
    #[pin]
    inner: F,
    state: MetricsState,
    start: Instant,
    method: Method,
    handler: Option<HandlerName>,
    path: Option<MatchedPath>,
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
        let resp = ready!(this.inner.poll(cx));

        let kv_method = KeyValue::new("http.request.method", this.method.to_string());
        // FIXME: get scheme from request
        let kv_scheme = KeyValue::new("url.scheme", "http");
        this.state
            .http_server
            .requests_active
            .add(-1, &[kv_method.clone(), kv_scheme.clone()]);

        let resp = resp?;
        let duration = this.start.elapsed().as_secs_f64();
        let status = resp.status().as_str().to_string();
        let response_size = resp.size_hint().upper().unwrap_or(0);

        let labels = [
            kv_method,
            kv_scheme,
            KeyValue::new("http.response.status_code", status),
            KeyValue::new(
                "http.route",
                this.path
                    .as_ref()
                    .map(|p| Cow::Owned(p.as_str().to_string()))
                    .unwrap_or(Cow::Borrowed("")),
            ),
            KeyValue::new(
                "uxum.handler",
                this.handler.map(|h| h.as_str()).unwrap_or(""),
            ),
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
        trace!("Metrics recorded");

        Poll::Ready(Ok(resp))
    }
}

///
// TODO: return Result
async fn get_metrics(metrics: State<MetricsState>) -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let mut buf = Vec::new();
    encoder
        .encode(&metrics.registry.gather(), &mut buf)
        .unwrap();
    buf
}
