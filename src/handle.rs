//! Handle object to start, stop and control the service.

use std::{net::SocketAddr, time::Duration};

use axum_server::{service::MakeService, Handle as AxumHandle};
use futures::{stream::FuturesUnordered, StreamExt, TryFutureExt};
use opentelemetry::{metrics::MeterProvider as _, trace::TracerProvider as _};
use opentelemetry_sdk::{
    metrics::SdkMeterProvider,
    trace::{SdkTracerProvider, Tracer},
};
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    builder::server::ServerBuilder, config::AppConfig, crypto::ensure_default_crypto_provider,
    errors::IoError, metrics::gather_runtime_metrics, notify::ServiceNotifier,
};

/// Error type returned by uxum handle.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HandleError {
    /// Error while setting up logging.
    #[error(transparent)]
    Logging(#[from] crate::logging::LoggingError),
    /// Error while setting up trace collection and propagation.
    #[error(transparent)]
    Tracing(#[from] crate::tracing::TracingError),
    /// Error while setting up metrics collection and propagation.
    #[error(transparent)]
    Metrics(#[from] crate::metrics::MetricsError),
    /// Error while building HTTP server.
    #[error(transparent)]
    ServerBuilder(#[from] crate::builder::server::ServerBuilderError),
    /// Error running HTTP server.
    #[error("HTTP server error: {0}")]
    Server(IoError),
    /// Error initializing crypto provider.
    #[error("Error initializing crypto provider")]
    InitTls,
    /// Error running HTTPS server.
    #[error("HTTPS server error: {0}")]
    TlsServer(IoError),
    /// Server task error.
    #[error("Server task error: {0}")]
    ServerTask(#[from] tokio::task::JoinError),
    /// No server is currently running.
    #[error("No server is currently running")]
    NotRunning,
    /// Custom error from application initialization.
    #[error("Custom error: {0}")]
    Custom(Box<dyn std::error::Error + Send + Sync>),
}

impl HandleError {
    /// Wrap custom application initialization error.
    #[must_use]
    pub fn custom<T>(err: T) -> Self
    where
        T: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Self::Custom(err.into())
    }
}

/// Handle for starting and controlling the server.
///
/// Unwritten logs will be flushed when dropping this object. This might help even in case of a
/// panic.
#[allow(dead_code)]
#[non_exhaustive]
pub struct Handle {
    /// Cancellation token for auxillary tasks.
    token: CancellationToken,
    /// Guards for [`tracing_appender::non_blocking::NonBlocking`].
    buf_guards: Vec<WorkerGuard>,
    /// Tracing pipeline.
    tracer: Option<Tracer>,
    /// Tracing provider.
    tracer_provider: Option<SdkTracerProvider>,
    /// Metrics provider.
    metrics_provider: Option<SdkMeterProvider>,
    /// Internal [`axum_server`] control handle.
    handle: AxumHandle,
    /// Service supervisor notification.
    notify: ServiceNotifier,
    /// Service supervisor notification task.
    service_watchdog: Option<JoinHandle<()>>,
    /// UNIX signal handler task.
    signal_handler: Option<JoinHandle<()>>,
    /// Plain HTTP server task.
    http_task: Option<JoinHandle<Result<(), HandleError>>>,
    /// HTTPS server task.
    https_task: Option<JoinHandle<Result<(), HandleError>>>,
    /// Runtime metrics recording task.
    rt_metrics_task: Option<JoinHandle<()>>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.token.cancel();
        if let Some(provider) = self.metrics_provider.take() {
            if let Err(err) = provider.force_flush() {
                eprintln!("Error flushing metrics: {err}");
            }
            if let Err(err) = provider.shutdown() {
                eprintln!("Error shutting down OTel metrics provider: {err}")
            }
        }
        if let Some(provider) = self.tracer_provider.take() {
            if let Err(err) = provider.force_flush() {
                eprintln!("Error flushing spans: {err}");
            }
            if let Err(err) = provider.shutdown() {
                eprintln!("Error shutting down OTel tracing provider: {err}")
            }
        }
    }
}

impl Handle {
    /// Set up background service tasks.
    fn prepare(&mut self, server: &ServerBuilder) -> Result<(), HandleError> {
        if self.signal_handler.is_none() {
            self.signal_handler = Some(server.spawn_signal_handler(self.handle.clone())?);
        }
        if self.service_watchdog.is_none() {
            self.service_watchdog = Some(tokio::spawn(self.notify.watchdog_task()));
        }
        Ok(())
    }

    /// Start axum server tasks.
    async fn start_servers<A>(&mut self, server: ServerBuilder, app: A) -> Result<(), HandleError>
    where
        A: MakeService<SocketAddr, http::Request<hyper::body::Incoming>>
            + tower::Service<SocketAddr>
            + Clone
            + Send
            + 'static,
        A::Response: tower::Service<http::Request<hyper::body::Incoming>>,
        A::MakeFuture: Send,
    {
        if server.has_tls_config() {
            ensure_default_crypto_provider();
            self.https_task = Some(tokio::spawn(
                server
                    .clone()
                    .build_tls()
                    .await?
                    .handle(self.handle.clone())
                    .serve(app.clone())
                    .map_err(|err| HandleError::TlsServer(err.into())),
            ));
        }
        self.http_task = Some(tokio::spawn(
            server
                .build()
                .await?
                .handle(self.handle.clone())
                .serve(app)
                .map_err(|err| HandleError::Server(err.into())),
        ));
        Ok(())
    }

    /// Start the server in the background.
    ///
    /// # Errors
    ///
    /// Returns `Err` if caught an error when initializing server tasks.
    pub async fn start<A>(&mut self, server: ServerBuilder, app: A) -> Result<(), HandleError>
    where
        A: MakeService<SocketAddr, http::Request<hyper::body::Incoming>>
            + tower::Service<SocketAddr>
            + Clone
            + Send
            + 'static,
        A::Response: tower::Service<http::Request<hyper::body::Incoming>>,
        A::MakeFuture: Send,
    {
        self.prepare(&server)?;
        self.start_servers(server, app).await?;
        self.notify.on_ready();
        Ok(())
    }

    /// Immediately shutdown the server.
    ///
    /// # Errors
    ///
    /// Returns `Err` if one of server tasks finished with an error.
    pub async fn shutdown(&mut self) -> Result<(), HandleError> {
        self.notify.on_shutdown();
        self.handle.shutdown();
        if let Some(task) = self.http_task.take() {
            task.await??;
        }
        if let Some(task) = self.https_task.take() {
            task.await??;
        }
        Ok(())
    }

    /// Gracefully shutdown the server, waiting for in-progress requests to finish.
    ///
    /// # Errors
    ///
    /// Returns `Err` if one of server tasks finished with an error.
    pub async fn graceful_shutdown(
        &mut self,
        graceful: Option<Duration>,
    ) -> Result<(), HandleError> {
        self.notify.on_shutdown();
        self.handle.graceful_shutdown(graceful);
        if let Some(task) = self.http_task.take() {
            task.await??;
        }
        if let Some(task) = self.https_task.take() {
            task.await??;
        }
        Ok(())
    }

    /// Immediately abort execution of the server.
    pub fn abort(&mut self) {
        self.notify.on_shutdown();
        if let Some(task) = self.http_task.take() {
            task.abort();
        }
        if let Some(task) = self.https_task.take() {
            task.abort();
        }
    }

    /// Start the server and block execution until one of the server tasks exits.
    ///
    /// Will gracefully shutdown remaining server tasks.
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// * Caught an error when initializing server tasks.
    /// * One of server tasks finished with an error.
    pub async fn run<A>(
        &mut self,
        server: ServerBuilder,
        app: A,
        graceful: Option<Duration>,
    ) -> Result<(), HandleError>
    where
        A: MakeService<SocketAddr, http::Request<hyper::body::Incoming>>
            + tower::Service<SocketAddr>
            + Clone
            + Send
            + 'static,
        A::Response: tower::Service<http::Request<hyper::body::Incoming>>,
        A::MakeFuture: Send,
    {
        self.start(server, app).await?;
        self.wait(graceful).await
    }

    /// Block execution until one of the server tasks exits.
    ///
    /// Will gracefully shutdown remaining server tasks.
    ///
    /// # Errors
    ///
    /// Returns `Err` if one of server tasks finished with an error.
    pub async fn wait(&mut self, graceful: Option<Duration>) -> Result<(), HandleError> {
        let http_fut = self.http_task.take();
        let https_fut = self.https_task.take();
        match (http_fut, https_fut) {
            (None, None) => Err(HandleError::NotRunning),
            (Some(task), None) => task.await?,
            (None, Some(task)) => task.await?,
            (Some(http), Some(https)) => {
                let mut tasks = FuturesUnordered::new();
                tasks.push(http);
                tasks.push(https);
                match tasks.next().await {
                    // Gracefully shutdown other tasks and return result of the one which exited
                    // first.
                    Some(ret) => {
                        self.handle.graceful_shutdown(graceful);
                        while let Some(other_ret) = tasks.next().await {
                            let _ = other_ret?;
                        }
                        ret?
                    }
                    // This should not happen normally.
                    None => Ok(()),
                }
            }
        }
    }
}

impl AppConfig {
    /// Create application control handle and initialize logging and tracing subsystems.
    ///
    /// Returns a guard that shouldn't be dropped as long as there is a need for these subsystems.
    ///
    /// You can start, stop, control and monitor application with this handle.
    ///
    /// Note that this call configures and finalizes logging, tracing and metrics subsystems
    /// so if you want to make changes to those programmatically - should do it before creating
    /// the [`Handle`].
    ///
    /// # Errors
    ///
    /// Returns `Err` if any part of initializing of tracing or logging subsystems ends with and
    /// error.
    pub async fn handle(&mut self) -> Result<Handle, HandleError> {
        let token = CancellationToken::new();
        let (registry, buf_guards) = self.logging.make_registry()?;
        let otel_res = self.otel_resource();
        let (tracer, tracer_provider) = if let Some(tcfg) = self.tracing.as_mut() {
            let tracer_provider = tcfg.build_provider(otel_res.clone()).await?;
            let tracer = tracer_provider.tracer("uxum");
            let layer = tcfg.build_layer(&tracer);
            registry.with(layer).init();
            opentelemetry::global::set_text_map_propagator(
                opentelemetry_sdk::propagation::TraceContextPropagator::default(),
            );
            (Some(tracer), Some(tracer_provider))
        } else {
            registry.init();
            (None, None)
        };
        let (metrics_provider, rt_metrics_task) = if let Some(mcfg) = self.metrics.as_ref() {
            let (metrics_provider, prom_exporter) = mcfg.build_provider(otel_res).await?;
            let meter = metrics_provider.meter("uxum");
            let metrics_state = mcfg.build_state(&meter, prom_exporter);
            let rt_task = tokio::spawn(gather_runtime_metrics(
                metrics_state.clone(),
                mcfg.runtime_metrics_interval,
                token.clone(),
            ));
            self.metrics_state = Some(metrics_state);
            opentelemetry::global::set_meter_provider(metrics_provider.clone());
            (Some(metrics_provider), Some(rt_task))
        } else {
            (None, None)
        };
        let handle = AxumHandle::new();
        let notify = ServiceNotifier::new();
        Ok(Handle {
            token,
            buf_guards,
            tracer,
            tracer_provider,
            metrics_provider,
            handle,
            notify,
            service_watchdog: None,
            signal_handler: None,
            http_task: None,
            https_task: None,
            rt_metrics_task,
        })
    }
}
