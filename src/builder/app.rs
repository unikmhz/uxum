use std::{
    collections::{BTreeMap, HashSet},
    convert::Infallible,
};

use axum::{
    body::{Body, Bytes, HttpBody},
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
use thiserror::Error;
use tower::{builder::ServiceBuilder, util::BoxCloneService, BoxError, Service};
use tower_http::{
    request_id::MakeRequestUuid,
    set_header::SetResponseHeaderLayer,
    trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
    LatencyUnit, ServiceBuilderExt,
};
use tracing::{debug, debug_span, info, info_span};

use crate::{
    apidoc::{ApiDocBuilder, ApiDocError},
    config::{AppConfig, HandlerConfig},
    layers::ext::HandlerName,
    metrics::{MetricsBuilder, MetricsError},
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
}

impl From<AppConfig> for AppBuilder {
    fn from(value: AppConfig) -> Self {
        Self { config: value }
    }
}

impl AppBuilder {
    /// Create new builder with default configuration
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn from_config(cfg: &AppConfig) -> Self {
        cfg.clone().into()
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
    pub fn build(mut self) -> Result<Router, AppBuilderError> {
        let _build_span = debug_span!("build_app").entered();
        let mut grouped: BTreeMap<&str, Vec<&dyn HandlerExt>> = BTreeMap::new();
        let mut rtr = Router::new();

        let otel_res = self.config.otel_resource();
        // TODO: export metrics state for application-defined metrics
        let metrics_state = self.config.metrics.build_state(otel_res.clone())?;
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
            debug!("handler recorded");
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
                        info!("skipping disabled handler");
                        continue;
                    }
                }
                method_rtr = handler.register_method(method_rtr, self.config.handlers.get(name));
                has_some = true;
                info!("handler registered");
            }
            if has_some {
                rtr = rtr.route(path, method_rtr);
            }
        }

        if let Some(ref mut api_doc) = self.config.api_doc {
            api_doc.set_app_defaults(
                self.config.app_name.as_deref(),
                self.config.app_version.as_deref(),
            );
            rtr = rtr.merge(api_doc.build_router()?);
        }

        let global_layers = ServiceBuilder::new()
            .set_x_request_id(MakeRequestUuid)
            .sensitive_headers([header::AUTHORIZATION])
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(DefaultMakeSpan::new().include_headers(true))
                    .on_request(DefaultOnRequest::new().level(tracing::Level::DEBUG))
                    .on_response(
                        DefaultOnResponse::new()
                            .level(tracing::Level::INFO)
                            .include_headers(true)
                            .latency_unit(LatencyUnit::Micros),
                    ),
            )
            .layer(metrics_state)
            .propagate_x_request_id()
            .layer(SetResponseHeaderLayer::if_not_present(
                header::SERVER,
                self.server_header(),
            ));
        // TODO: DefaultBodyLimit (configurable)
        let final_rtr = rtr.layer(global_layers);
        info!("finished building application");
        Ok(final_rtr)
    }

    /// Generate a value to be used in HTTP Server header
    #[must_use]
    fn server_header(&self) -> Option<HeaderValue> {
        const UXUM_PRODUCT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
        if let Some(app_name) = &self.config.app_name {
            let val = if let Some(app_version) = &self.config.app_version {
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

/// Apply standard layer stack to provided handler function
#[must_use]
pub fn apply_layers<X, S, T, U>(
    hext: &X,
    handler: S,
    conf: Option<&HandlerConfig>,
) -> BoxCloneService<Request<T>, Response<Body>, Infallible>
where
    X: HandlerExt,
    S: Service<Request<T>, Response = Response<U>> + Send + Sync + Clone + 'static,
    S::Future: Send,
    S::Error: std::error::Error + Send + Sync,
    T: Send + 'static,
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
