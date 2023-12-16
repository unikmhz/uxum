use std::collections::BTreeMap;

use axum::{
    body::Body,
    error_handling::HandleErrorLayer,
    http::{
        header::{self, HeaderValue},
        StatusCode,
    },
    response::IntoResponse,
    routing::{MethodRouter, Router},
    Extension,
};
use hyper::{Request, Response};
use okapi::{openapi3, schemars::gen::SchemaGenerator};
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
};

/// Error type used in app builder
#[derive(Debug, Error)]
pub enum AppBuilderError {
    #[error(transparent)]
    ApiDoc(#[from] ApiDocError),
    #[error(transparent)]
    Metrics(#[from] MetricsError),
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
        Default::default()
    }

    /// Set short name of an application
    ///
    /// Whitespace is not allowed, as this value is used in Server: HTTP header, among other
    /// things.
    #[must_use]
    pub fn with_app_name(mut self, app_name: impl ToString) -> Self {
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
        self.config.metrics = Some(metrics);
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

    /// Configure metrics builder
    ///
    /// This gets the builder from configuration, or creates a new default one if configuration
    /// wasn't provided. Next it passes the builder to the provided callback function for
    /// additional customization.
    pub fn configure_metrics<F>(&mut self, modifier: F)
    where
        F: FnOnce(MetricsBuilder) -> MetricsBuilder,
    {
        self.config.metrics = Some(modifier(self.config.metrics.take().unwrap_or_default()));
    }

    /// Build top-level Axum router
    pub fn build(mut self) -> Result<Router, AppBuilderError> {
        let _span = debug_span!("build_app").entered();
        let mut grouped: BTreeMap<&str, Vec<&dyn HandlerExt>> = BTreeMap::new();
        let mut rtr = Router::new();

        let metrics = match &self.config.metrics {
            Some(builder) => {
                // TODO: external state for application-defined metrics
                let state = builder.build_state()?;
                // TODO: set_app_defaults
                rtr = rtr.merge(state.build_router());
                Some(state)
            }
            None => None,
        };

        // TODO: error/panic on duplicate handler names
        for handler in inventory::iter::<&dyn HandlerExt> {
            let _span = debug_span!("iter_handler", name = handler.name()).entered();
            grouped
                .entry(handler.path())
                .and_modify(|handlers| handlers.push(*handler))
                .or_insert_with(|| vec![*handler]);
            debug!("Handler recorded");
        }
        for (path, handlers) in grouped.into_iter() {
            let _span = info_span!("register_path", path).entered();
            let mut has_some = false;
            let mut mrtr: MethodRouter<(), Body, BoxError> = MethodRouter::new();
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
                mrtr = handler.register_method(mrtr, self.config.handlers.get(name));
                has_some = true;
                info!("Handler registered");
            }
            if has_some {
                // FIXME: remove multiple error handling layers
                let path_layers = ServiceBuilder::new()
                    .layer(HandleErrorLayer::new(error_handler))
                    .option_layer(metrics.clone())
                    .layer(HandleErrorLayer::new(error_handler));
                rtr = rtr.route(path, mrtr.layer(path_layers));
            }
        }

        if self.config.api_doc.is_some() {
            let app_name = self.app_name.clone();
            let app_version = self.app_version.clone();
            if let Some(ref mut api_doc) = self.config.api_doc {
                api_doc.set_app_defaults(app_name, app_version);
                rtr = rtr.merge(api_doc.build_router()?);
            }
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
            .layer(SetResponseHeaderLayer::if_not_present(
                header::SERVER,
                self.server_header(),
            ));
        // TODO: DefaultBodyLimit (configurable)
        // TODO: SetSensitiveRequestHeadersLayer
        let rtr = rtr.layer(global_layers);
        info!("Finished building application");
        Ok(rtr)
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
) -> BoxCloneService<Request<T>, S::Response, BoxError>
where
    X: HandlerExt,
    S: Service<Request<T>, Response = Response<U>> + Send + Sync + Clone + 'static,
    S::Future: Send,
    S::Error: std::error::Error + Send + Sync,
    T: Send + Sync + 'static,
{
    ServiceBuilder::new()
        .boxed_clone()
        .layer(Extension(HandlerName::new(hext.name())))
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
    fn register_method(
        &self,
        mrtr: MethodRouter<(), Body, BoxError>,
        cfg: Option<&HandlerConfig>,
    ) -> MethodRouter<(), Body, BoxError>;
    fn openapi_spec(&self, gen: &mut SchemaGenerator) -> openapi3::Operation;
}

inventory::collect!(&'static dyn HandlerExt);
