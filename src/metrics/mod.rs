//! Subsystem to gather and export application metrics.

use std::{
    borrow::Cow,
    future::Future,
    ops::Deref,
    pin::Pin,
    sync::Arc,
    task::{ready, Context, Poll},
    time::{Duration, Instant},
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
    metrics::{Counter, Gauge, Histogram, Meter, UpDownCounter},
    KeyValue,
};
use opentelemetry_otlp::{ExporterBuildError, WithExportConfig, WithTonicConfig};
use opentelemetry_prometheus_text_exporter::PrometheusExporter;
use opentelemetry_sdk::{
    metrics::{SdkMeterProvider, Temporality},
    Resource,
};
use pin_project::pin_project;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use tower::{Layer, Service};
use tracing::{debug_span, trace, Instrument};
use url::Url;

use crate::{
    crypto::TonicTlsConfig,
    errors::{self, IoError},
    layers::ext::HandlerName,
    telemetry::OtlpProtocol,
};

/// Error type used in metrics subsystem.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MetricsError {
    /// OpenTelemetry metrics error.
    #[error("OTel metrics setup error: {0}")]
    OpenTelemetry(#[from] ExporterBuildError),
    /// Error loading files in configuration.
    #[error("Error loading files in configuration: {0}")]
    ConfigRead(IoError),
    /// Error collecting metrics.
    #[error("Error collecting Prometheus metrics: {0}")]
    Prometheus(IoError),
    /// Metrics subsystem is misconfigured.
    #[error("Metrics subsystem is misconfigured")]
    RuntimeConfig,
}

impl IntoResponse for MetricsError {
    fn into_response(self) -> Response {
        problemdetails::new(StatusCode::INTERNAL_SERVER_ERROR)
            .with_type(errors::TAG_UXUM_METRICS)
            .with_title(self.to_string())
            .into_response()
    }
}

/// Configuration and builder for metrics subsystem.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct MetricsBuilder {
    /// List of exporters to supply metrics to.
    #[serde(
        default = "MetricsBuilder::default_exporters",
        skip_serializing_if = "Vec::is_empty"
    )]
    exporters: Vec<MetricsExporterConfig>,
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
    /// Time interval between recording runtime metrics.
    #[serde(
        default = "MetricsBuilder::default_runtime_metrics_interval",
        with = "humantime_serde"
    )]
    pub runtime_metrics_interval: Duration,
}

impl Default for MetricsBuilder {
    fn default() -> Self {
        Self {
            exporters: Self::default_exporters(),
            duration_buckets: Self::default_duration_buckets(),
            size_buckets: Self::default_size_buckets(),
            runtime_metrics_interval: Self::default_runtime_metrics_interval(),
        }
    }
}

impl From<MetricsExporterConfig> for MetricsBuilder {
    fn from(value: MetricsExporterConfig) -> Self {
        Self {
            exporters: vec![value],
            ..Default::default()
        }
    }
}

impl MetricsBuilder {
    /// Default value for [`Self::exporters`].
    #[must_use]
    #[inline]
    fn default_exporters() -> Vec<MetricsExporterConfig> {
        vec![MetricsExporterConfig::default()]
    }

    /// Default value for [`Self::duration_buckets`].
    #[must_use]
    #[inline]
    fn default_duration_buckets() -> Vec<f64> {
        [
            0.0_f64, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0,
            2.5, 5.0, 7.5, 10.0, 30.0,
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

    /// Default value for [`Self::runtime_metrics_interval`].
    #[must_use]
    #[inline]
    fn default_runtime_metrics_interval() -> Duration {
        Duration::from_secs(15)
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

    /// Set interval to collect runtime metrics.
    #[must_use]
    pub fn with_runtime_metrics_interval(mut self, interval: Duration) -> Self {
        self.runtime_metrics_interval = interval;
        self
    }

    /// Build OpenTelemetry metrics provider.
    ///
    /// Returns built provider, and a Prometheus exporter, if it was mentioned in configuration.
    /// You should use this exporter from within a handler to generate metrics in text format.
    ///
    /// # Errors
    ///
    /// Returns `Err` if metrics exporter and/or processor cannot be installed for some reason.
    pub async fn build_provider(
        &self,
        resource: Resource,
    ) -> Result<(SdkMeterProvider, Option<PrometheusExporter>), MetricsError> {
        let span = debug_span!("build_tracing_provider");
        async {
            let mut prom = None;
            let mut provider = SdkMeterProvider::builder().with_resource(resource);
            for exp_cfg in &self.exporters {
                match exp_cfg {
                    MetricsExporterConfig::Prometheus(cfg) => {
                        let exp = cfg.build_exporter();
                        prom = Some(exp.clone());
                        provider = provider.with_reader(exp);
                    }
                    MetricsExporterConfig::Otlp(cfg) => {
                        let exp = cfg.build_exporter().await?;
                        provider = provider.with_periodic_exporter(exp);
                    }
                }
            }
            Ok((provider.build(), prom))
        }
        .instrument(span)
        .await
    }

    /// Build metrics state object.
    pub fn build_state(&self, meter: &Meter, prom: Option<PrometheusExporter>) -> MetricsState {
        // HTTP server metrics.
        let request_duration = meter
            .f64_histogram("http.server.request.duration")
            .with_unit("s")
            .with_boundaries(self.duration_buckets.clone())
            .with_description("The HTTP request latencies in seconds.")
            .build();
        let requests_total = meter
            .u64_counter("http.server.requests")
            .with_description(
                "How many HTTP requests processed, partitioned by status code and HTTP method.",
            )
            .build();
        let requests_active = meter
            .i64_up_down_counter("http.server.active_requests")
            .with_description("The number of active HTTP requests.")
            .build();
        let request_body_size = meter
            .u64_histogram("http.server.request.body.size")
            .with_unit("By")
            .with_boundaries(self.size_buckets.clone())
            .with_description("The HTTP request body sizes in bytes.")
            .build();
        let response_body_size = meter
            .u64_histogram("http.server.response.body.size")
            .with_unit("By")
            .with_boundaries(self.size_buckets.clone())
            .with_description("The HTTP reponse body sizes in bytes.")
            .build();
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
            .with_unit("s")
            .with_boundaries(self.duration_buckets.clone())
            .with_description("The HTTP request latencies in seconds.")
            .build();
        let requests_total = meter
            .u64_counter("http.client.requests")
            .with_description(
                "How many HTTP requests processed, partitioned by status code and HTTP method.",
            )
            .build();
        let requests_rejected = meter
            .u64_counter("http.client.rejected_requests")
            .with_description("Rejected requests due to open circuit breaker.")
            .build();
        let requests_errored = meter
            .u64_counter("http.client.errored_requests")
            .with_description("Other errors when trying to send a request.")
            .build();
        let requests_active = meter
            .i64_up_down_counter("http.client.active_requests")
            .with_description("The number of active HTTP requests.")
            .build();
        let request_body_size = meter
            .u64_histogram("http.client.request.body.size")
            .with_unit("By")
            .with_boundaries(self.size_buckets.clone())
            .with_description("The HTTP request body sizes in bytes.")
            .build();
        let response_body_size = meter
            .u64_histogram("http.client.response.body.size")
            .with_unit("By")
            .with_boundaries(self.size_buckets.clone())
            .with_description("The HTTP reponse body sizes in bytes.")
            .build();
        let http_client = HttpClientMetricsInner {
            request_duration,
            requests_total,
            requests_rejected,
            requests_errored,
            requests_active,
            request_body_size,
            response_body_size,
        };
        let http_client = HttpClientMetrics(Arc::new(http_client));

        // Tokio runtime metrics.
        let num_workers = meter
            .u64_gauge("runtime.workers")
            .with_description("Number of worker threads used by the runtime.")
            .build();
        let num_alive_tasks = meter
            .u64_gauge("runtime.alive_tasks")
            .with_description("Current number of alive tasks in the runtime.")
            .build();
        let global_queue_depth = meter
            .u64_gauge("runtime.global_queue_depth")
            .with_description("Number of tasks currently scheduled in the runtime’s global queue.")
            .build();
        let runtime = RuntimeMetrics {
            num_workers,
            num_alive_tasks,
            global_queue_depth,
        };

        MetricsState {
            http_server,
            http_client,
            runtime,
            prom,
        }
    }

    /// Build Axum router containing all metrics methods.
    pub fn build_router(&self, metrics_state: &MetricsState) -> Router {
        let _span = debug_span!("build_metrics_router").entered();
        let mut rtr = Router::new();
        for exp_cfg in &self.exporters {
            if let MetricsExporterConfig::Prometheus(cfg) = exp_cfg {
                rtr = rtr.route(&cfg.path, routing::get(get_prom_metrics))
            }
        }
        rtr.with_state(metrics_state.clone())
    }
}

/// Configuration for OpenTelemetry metrics exporter.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
enum MetricsExporterConfig {
    /// Export metrics via a local handler that returns collected metrics in Prometheus text
    /// format.
    #[serde(alias = "prom")]
    Prometheus(PrometheusMetricsExporterConfig),
    /// Export metrics via pushing to a remote OTLP endpoint.
    Otlp(OtlpMetricsExporterConfig),
}

impl Default for MetricsExporterConfig {
    fn default() -> Self {
        Self::Prometheus(Default::default())
    }
}

/// Configuration for OpenTelemetry Prometheus metrics exporter.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
struct PrometheusMetricsExporterConfig {
    /// URL path for metrics prometheus exporter endpoint.
    #[serde(default = "PrometheusMetricsExporterConfig::default_path")]
    path: String,
    /// Use automatic unit suffixes (e.g., `_seconds`, `_bytes`).
    #[serde(default = "crate::util::default_true")]
    with_units: bool,
    /// Use `_total` suffix on counter metrics.
    #[serde(default = "crate::util::default_true")]
    with_counter_suffixes: bool,
    /// Generate `target_info` metric from resource attributes.
    #[serde(default = "crate::util::default_true")]
    with_target_info: bool,
    /// Generate `otel_scope_info` metric with instrumentation scope labels.
    #[serde(default = "crate::util::default_true")]
    with_scope_info: bool,
}

impl Default for PrometheusMetricsExporterConfig {
    fn default() -> Self {
        Self {
            path: Self::default_path(),
            with_units: true,
            with_counter_suffixes: true,
            with_target_info: true,
            with_scope_info: true,
        }
    }
}

impl PrometheusMetricsExporterConfig {
    /// Default value for [`Self::path`].
    #[must_use]
    #[inline]
    fn default_path() -> String {
        String::from("/metrics")
    }

    /// Build exporter.
    #[must_use]
    fn build_exporter(&self) -> PrometheusExporter {
        let mut builder = PrometheusExporter::builder();
        if !self.with_units {
            builder = builder.without_units();
        }
        if !self.with_counter_suffixes {
            builder = builder.without_counter_suffixes();
        }
        if !self.with_target_info {
            builder = builder.without_target_info();
        }
        if !self.with_scope_info {
            builder = builder.without_scope_info();
        }
        builder.build()
    }
}

/// Configuration for OpenTelemetry OTLP metrics exporter.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
struct OtlpMetricsExporterConfig {
    /// Metrics collector endpoint URL.
    #[serde(default = "OtlpMetricsExporterConfig::default_endpoint")]
    endpoint: Url,
    /// Protocol to use when exporting data.
    #[serde(default, alias = "format")]
    protocol: OtlpProtocol,
    /// Timeout for an outbound exporter request.
    #[serde(
        default = "OtlpMetricsExporterConfig::default_timeout",
        with = "humantime_serde"
    )]
    timeout: Duration,
    /// Default temporality for collected metrics.
    #[serde(default)]
    temporality: MetricsTemporality,
    /// TLS configuration for exporter.
    #[serde(default)]
    tls: TonicTlsConfig,
}

impl Default for OtlpMetricsExporterConfig {
    fn default() -> Self {
        Self {
            endpoint: Self::default_endpoint(),
            protocol: OtlpProtocol::default(),
            timeout: Self::default_timeout(),
            temporality: MetricsTemporality::default(),
            tls: TonicTlsConfig::default(),
        }
    }
}

impl OtlpMetricsExporterConfig {
    /// Default value for [`Self::endpoint`].
    #[must_use]
    #[inline]
    #[allow(clippy::unwrap_used)]
    fn default_endpoint() -> Url {
        // TODO: check correctness using a unit test.
        Url::parse("http://localhost:9090/api/v1/otlp/v1/metrics").unwrap()
    }

    /// Default value for [`Self::timeout`].
    #[must_use]
    #[inline]
    fn default_timeout() -> Duration {
        opentelemetry_otlp::OTEL_EXPORTER_OTLP_TIMEOUT_DEFAULT
    }

    /// Try building exporter.
    ///
    /// # Errors
    ///
    /// Returns `Err` if some files required to properly initialize exporter could not be loaded.
    async fn build_exporter(&self) -> Result<opentelemetry_otlp::MetricExporter, MetricsError> {
        opentelemetry_otlp::MetricExporter::builder()
            .with_tonic()
            .with_endpoint(self.endpoint.to_string())
            .with_protocol(self.protocol.into())
            .with_timeout(self.timeout)
            .with_tls_config(
                self.tls
                    .to_tonic_config()
                    .await
                    .map_err(|err| MetricsError::ConfigRead(err.into()))?,
            )
            .with_temporality(self.temporality.into())
            .build()
            .map_err(Into::into)
    }
}

/// Defines the window that an aggregation was calculated over.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Hash, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum MetricsTemporality {
    /// A measurement interval that continues to expand forward in time from a
    /// starting point.
    ///
    /// New measurements are added to all previous measurements since a start time.
    #[default]
    Cumulative,

    /// A measurement interval that resets each cycle.
    ///
    /// Measurements from one cycle are recorded independently, measurements from
    /// other cycles do not affect them.
    Delta,

    /// Configures Synchronous Counter and Histogram instruments to use
    /// Delta aggregation temporality, which allows them to shed memory
    /// following a cardinality explosion, thus use less memory.
    LowMemory,
}

impl From<MetricsTemporality> for Temporality {
    fn from(value: MetricsTemporality) -> Self {
        match value {
            MetricsTemporality::Cumulative => Self::Cumulative,
            MetricsTemporality::Delta => Self::Delta,
            MetricsTemporality::LowMemory => Self::LowMemory,
        }
    }
}

/// Metrics state [`tower`] layer.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct MetricsState {
    /// HTTP server metrics.
    http_server: HttpServerMetrics,
    /// HTTP client metrics.
    http_client: HttpClientMetrics,
    /// Tokio runtime metrics.
    runtime: RuntimeMetrics,
    /// Prometheus exporter.
    prom: Option<PrometheusExporter>,
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
    /// Rejected requests due to open circuit breaker.
    pub requests_rejected: Counter<u64>,
    /// Other errors when trying to send a request.
    pub requests_errored: Counter<u64>,
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
    num_workers: Gauge<u64>,
    /// Current number of alive tasks in the runtime.
    ///
    /// This counter increases when a task is spawned and decreases when a task exits.
    num_alive_tasks: Gauge<u64>,
    /// Number of tasks currently scheduled in the runtime’s global queue.
    ///
    /// Tasks that are spawned or notified from a non-runtime thread are scheduled using the runtime’s
    /// global queue. This metric returns the current number of tasks pending in the global queue.
    /// As such, the returned value may increase or decrease as new tasks are scheduled and processed.
    global_queue_depth: Gauge<u64>,
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
    /// Get HTTP client metrics state object.
    #[must_use]
    pub fn client_metrics(&self, name: impl AsRef<str>) -> ClientMetricsState {
        ClientMetricsState {
            name: name.as_ref().to_string(),
            metrics: self.http_client.clone(),
        }
    }

    /// Get metrics in Prometheus text format, or `None` if no Prometheus exporter was configured.
    ///
    /// # Errors
    ///
    /// Returns `Err` if encountered an error while collecting metrics.
    pub fn export_text(&self) -> Result<Option<Vec<u8>>, std::io::Error> {
        if let Some(exporter) = &self.prom {
            let mut buf = Vec::with_capacity(256);
            exporter.export(&mut buf)?;
            Ok(Some(buf))
        } else {
            Ok(None)
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
        // TODO: fix once https://github.com/tokio-rs/axum/issues/2504 is released.
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

/// Periodic task to record Tokio runtime metrics.
pub(crate) async fn gather_runtime_metrics(
    metrics: MetricsState,
    period: Duration,
    cancel: CancellationToken,
) {
    let mut interval = tokio::time::interval(period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = interval.tick() => {
                let rt_metrics = tokio::runtime::Handle::current().metrics();
                metrics
                    .runtime
                    .num_workers
                    .record(rt_metrics.num_workers() as u64, &[]);
                metrics
                    .runtime
                    .num_alive_tasks
                    .record(rt_metrics.num_alive_tasks() as u64, &[]);
                metrics
                    .runtime
                    .global_queue_depth
                    .record(rt_metrics.global_queue_depth() as u64, &[]);
            }
        }
    }
}

/// Method handler to generate metrics.
async fn get_prom_metrics(metrics: State<MetricsState>) -> Result<impl IntoResponse, MetricsError> {
    // Serialize metrics.
    let buf = match metrics.export_text() {
        Ok(Some(buf)) => buf,
        Ok(None) => return Err(MetricsError::RuntimeConfig),
        Err(err) => return Err(MetricsError::Prometheus(err.into())),
    };
    Ok(([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], buf))
}
