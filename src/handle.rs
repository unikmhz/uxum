use std::net::SocketAddr;

use axum::Router;
use axum_server::Handle as AxumHandle;
use opentelemetry_sdk::trace::Tracer;
use thiserror::Error;
use tokio::task::JoinHandle;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    builder::server::ServerBuilder, config::AppConfig, errors::IoError, notify::ServiceNotifier,
};

/// Error type returned by uxum handle
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HandleError {
    /// Error while setting up logging
    #[error(transparent)]
    Logging(#[from] crate::logging::LoggingError),
    /// Error while setting up trace collection and propagation
    #[error(transparent)]
    Tracing(#[from] crate::tracing::TracingError),
    ///
    #[error(transparent)]
    ServerBuilder(#[from] crate::builder::server::ServerBuilderError),
    ///
    #[error("Server error: {0}")]
    Server(IoError),
}

/// Handle for starting and controlling the server
///
/// Unwritten logs will be flushed when dropping this object. This might help even in case of a
/// panic.
#[allow(dead_code)]
pub struct Handle {
    /// Guards for [`tracing_appender::non_blocking::NonBlocking`]
    buf_guards: Vec<WorkerGuard>,
    /// Tracing pipeline
    tracer: Option<Tracer>,
    ///
    handle: AxumHandle,
    /// Service supervisor notification
    notify: ServiceNotifier,
    ///
    service_watchdog: Option<JoinHandle<()>>,
    ///
    signal_handler: Option<JoinHandle<()>>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        if let Some(provider) = self.tracer.as_ref().and_then(|t| t.provider()) {
            for res in provider.force_flush() {
                if let Err(err) = res {
                    eprintln!("Error flushing spans: {err}");
                }
            }
        }
    }
}

impl Handle {
    pub async fn start(&mut self, server: ServerBuilder, app: Router) -> Result<(), HandleError> {
        if self.signal_handler.is_none() {
            self.signal_handler = Some(server.spawn_signal_handler(self.handle.clone())?);
        }
        self.notify.on_ready();
        if self.service_watchdog.is_none() {
            self.service_watchdog = Some(tokio::spawn(self.notify.watchdog_task()));
        }
        // TODO: start watchdog task
        // TODO: spawn server as distinct task and return Ok(())
        server
            .build()
            .await?
            .handle(self.handle.clone())
            // axum::ServiceExt
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .map_err(|err| HandleError::Server(err.into()))
    }
}

impl AppConfig {
    /// Initialize logging and tracing subsystems
    ///
    /// Returns a guard that shouldn't be dropped as long as there is a need for these subsystems.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any part of initializing of tracing or logging subsystems ends with and
    /// error.
    pub fn handle(&mut self) -> Result<Handle, HandleError> {
        let (registry, buf_guards) = self.logging.make_registry()?;
        let otel_res = self.otel_resource();
        let tracer = if let Some(tcfg) = self.tracing.as_mut() {
            let tracer = tcfg.build_pipeline(otel_res)?;
            let layer = tcfg.build_layer(&tracer);
            registry.with(layer).init();
            opentelemetry::global::set_text_map_propagator(
                opentelemetry_sdk::propagation::TraceContextPropagator::default(),
            );
            Some(tracer)
        } else {
            registry.init();
            None
        };
        let handle = AxumHandle::new();
        let notify = ServiceNotifier::new();
        Ok(Handle {
            buf_guards,
            tracer,
            handle,
            notify,
            service_watchdog: None,
            signal_handler: None,
        })
    }
}
