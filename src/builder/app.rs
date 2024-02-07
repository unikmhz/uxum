use std::{
    collections::{BTreeMap, HashSet},
    convert::Infallible,
};

use axum::{
    body::{BoxBody, Bytes, HttpBody},
    error_handling::HandleErrorLayer,
    http::{
        header::{self, HeaderValue},
        StatusCode,
    },
    response::IntoResponse,
    routing::{MethodRouter, Router},
};
use hyper::{Request, Response};
use okapi::{openapi3, schemars::gen::SchemaGenerator};
use opentelemetry_sdk::trace::Tracer;
use thiserror::Error;
use tower::{builder::ServiceBuilder, util::BoxCloneService, BoxError, Service};
use tower_http::{
    set_header::SetResponseHeaderLayer,
    trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
    LatencyUnit,
};
use tracing::{debug, debug_span, info, info_span};

use crate::{
    apidoc::{ApiDocBuilder, ApiDocError},
    config::{AppConfig, HandlerConfig},
    layers::ext::HandlerName,
    metrics::{MetricsBuilder, MetricsError},
    otel::otel_resource,
    tracing::TracingError,
    util::ResponseExtension,
};

/// Error type used in app builder
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AppBuilderError {
    #[error(transparent)]
    ApiDoc(#[from] ApiDocError),
    #[error(transparent)]
    Metrics(#[from] MetricsError),
    #[error(transparent)]
    Tracing(#[from] TracingError),
    #[error("Duplicate handler name: {0}")]
    DuplicateHandlerName(&'static str),
}

/// Builder for application routes
#[derive(Debug, Default)]
pub struct AppBuilder {
    /// Application configuration
    config: AppConfig,
    /// Short application name
    app_name: Option<String>,
    /// Application version
    app_version: Option<String>,
}

impl From<AppConfig> for AppBuilder {
    fn from(value: AppConfig) -> Self {
        Self {
            config: value,
            app_name: None,
            app_version: None,
        }
    }
}

impl AppBuilder {
    /// Create new builder with default configuration
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set short name of an application
    ///
    /// Whitespace is not allowed, as this value is used in Server: HTTP header, among other
    /// things.
    #[must_use]
    pub fn with_app_name(mut self, app_name: impl ToString) -> Self {
        // TODO: mybe check for value correctness?
        self.app_name = Some(app_name.to_string());
        self
    }

    /// Set application version
    ///
    /// Preferably in semver format. Whitespace is not allowed, as this value is used in Server:
    /// HTTP header, among other things.
    #[must_use]
    pub fn with_app_version(mut self, app_version: impl ToString) -> Self {
        self.app_version = Some(app_version.to_string());
        self
    }

    /// Set used API doc builder
    ///
    /// The builder must be configured prior to passing it to this method. This enables OpenAPI
    /// spec generation, and an (optional) RapiDoc UI.
    ///
    /// Alternatively, you can include API doc configuration in [`AppConfig::api_doc`] section.
    #[must_use]
    pub fn with_api_doc(mut self, api_doc: ApiDocBuilder) -> Self {
        self.config.api_doc = Some(api_doc);
        self
    }

    /// Set used metrics builder
    ///
    /// The builder must be configured prior to passing it to this method. This enables gathering
    /// of handler execution metrics, as well as an exporter HTTP endpoint.
    ///
    /// Alternatively, you can include metrics configuration in [`AppConfig::metrics`] section.
    #[must_use]
    pub fn with_metrics(mut self, metrics: MetricsBuilder) -> Self {
        self.config.metrics = metrics;
        self
    }

    /// Configure metrics builder
    ///
    /// This gets the builder from configuration, and then passes it to the provided callback
    /// function for additional customization.
    #[must_use]
    pub fn with_configured_metrics<F>(mut self, modifier: F) -> Self
    where
        F: FnOnce(MetricsBuilder) -> MetricsBuilder,
    {
        self.config.metrics = modifier(self.config.metrics);
        self
    }

    /// Configure API doc builder
    ///
    /// This gets the builder from configuration, or creates a new default one if configuration
    /// wasn't provided. Next it passes the builder to the provided callback function for
    /// additional customization.
    pub fn configure_api_doc<F>(&mut self, modifier: F)
    where
        F: FnOnce(ApiDocBuilder) -> ApiDocBuilder,
    {
        self.config.api_doc = Some(modifier(self.config.api_doc.take().unwrap_or_default()));
    }

    /// Build top-level Axum router
    ///
    /// # Errors
    ///
    /// Returns `Err` if some part of application setup did not succeed, or when there are
    /// conflicting handlers defined in application code.
    pub fn build(mut self) -> Result<(Router, Option<Tracer>), AppBuilderError> {
        let _build_span = debug_span!("build_app").entered();
        let mut grouped: BTreeMap<&str, Vec<&dyn HandlerExt>> = BTreeMap::new();
        let mut rtr = Router::new();

        let otel_res = otel_resource(
            self.config.otel.detector_timeout,
            None::<String>,
            self.app_name.as_deref(),
            self.app_version.as_deref(),
        );
        let tracer = self
            .config
            .tracing
            .as_ref()
            .map(|trace_cfg| trace_cfg.build_pipeline(otel_res.clone()))
            .transpose()?;
        // TODO: export metrics state for application-defined metrics
        let metrics_state = self.config.metrics.build_state(otel_res)?;
        if self.config.metrics.is_enabled() {
            rtr = rtr.merge(metrics_state.build_router());
        }

        let mut handler_names = HashSet::new();
        for handler in inventory::iter::<&dyn HandlerExt> {
            let name = handler.name();
            let _record_span = debug_span!("iter_handler", name).entered();
            if !handler_names.insert(name) {
                return Err(AppBuilderError::DuplicateHandlerName(name));
            }
            grouped
                .entry(handler.path())
                .and_modify(|handlers| handlers.push(*handler))
                .or_insert_with(|| vec![*handler]);
            debug!("Handler recorded");
        }
        for (path, handlers) in grouped {
            let _register_span = info_span!("register_path", path).entered();
            let mut has_some = false;
            let mut method_rtr = MethodRouter::new();
            for handler in handlers {
                let name = handler.name();
                let _span =
                    info_span!("register_handler", name, method = ?handler.method()).entered();
                if let Some(cfg) = self.config.handlers.get(name) {
                    if cfg.disabled {
                        info!("Skipping disabled handler");
                        continue;
                    }
                }
                method_rtr = handler.register_method(method_rtr, self.config.handlers.get(name));
                has_some = true;
                info!("Handler registered");
            }
            if has_some {
                rtr = rtr.route(path, method_rtr);
            }
        }

        if let Some(ref mut api_doc) = self.config.api_doc {
            api_doc.set_app_defaults(self.app_name.as_deref(), self.app_version.as_deref());
            rtr = rtr.merge(api_doc.build_router()?);
        }

        let global_layers = ServiceBuilder::new()
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(DefaultMakeSpan::new().include_headers(true))
                    .on_request(DefaultOnRequest::new().level(tracing::Level::DEBUG))
                    .on_response(
                        DefaultOnResponse::new()
                            .level(tracing::Level::INFO)
                            .latency_unit(LatencyUnit::Micros),
                    ),
            )
            .layer(metrics_state)
            .layer(SetResponseHeaderLayer::if_not_present(
                header::SERVER,
                self.server_header(),
            ));
        // TODO: DefaultBodyLimit (configurable)
        // TODO: SetSensitiveRequestHeadersLayer
        let final_rtr = rtr.layer(global_layers);
        info!("Finished building application");
        Ok((final_rtr, tracer))
    }

    /// Generate a value to be used in HTTP Server header
    #[must_use]
    fn server_header(&self) -> Option<HeaderValue> {
        const UXUM_PRODUCT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
        if let Some(app_name) = &self.app_name {
            let val = if let Some(app_version) = &self.app_version {
                let app_product = [app_name.as_str(), app_version.as_str()].join("/");
                [&app_product, UXUM_PRODUCT].join(" ")
            } else {
                [app_name, UXUM_PRODUCT].join(" ")
            };
            HeaderValue::from_str(&val).ok()
        } else {
            HeaderValue::from_str(UXUM_PRODUCT).ok()
        }
    }
}

// FIXME: write proper handler
async fn error_handler(err: BoxError) -> impl IntoResponse {
    (StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {err}"))
}

///
#[must_use]
pub fn apply_layers<X, S, T, U>(
    hext: &X,
    handler: S,
    conf: Option<&HandlerConfig>,
) -> BoxCloneService<Request<T>, Response<BoxBody>, Infallible>
where
    X: HandlerExt,
    S: Service<Request<T>, Response = Response<U>> + Send + Sync + Clone + 'static,
    S::Future: Send,
    S::Error: std::error::Error + Send + Sync,
    T: Send + Sync + 'static,
    U: HttpBody<Data = Bytes> + Send + 'static,
    <U as HttpBody>::Error: std::error::Error + Send + Sync,
{
    ServiceBuilder::new()
        .boxed_clone()
        .layer(HandleErrorLayer::new(error_handler))
        .layer(ResponseExtension(HandlerName::new(hext.name())))
        .option_layer(
            conf.and_then(|cfg| cfg.buffer.as_ref())
                .map(|lcfg| lcfg.make_layer()),
        )
        .option_layer(
            conf.and_then(|cfg| cfg.rate_limit.as_ref())
                .map(|rcfg| rcfg.make_layer()),
        )
        // TODO: circuit_breaker
        // TODO: throttle
        // TODO: timeout
        // TODO: roles
        .service(handler)
}

pub trait HandlerExt: Sync {
    fn name(&self) -> &'static str;
    fn path(&self) -> &'static str;
    fn spec_path(&self) -> &'static str;
    fn method(&self) -> http::Method;
    fn register_method(&self, mrtr: MethodRouter, cfg: Option<&HandlerConfig>) -> MethodRouter;
    fn openapi_spec(&self, gen: &mut SchemaGenerator) -> openapi3::Operation;
}

inventory::collect!(&'static dyn HandlerExt);
