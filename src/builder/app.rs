//! [`axum`] application builder.

use std::{
    any::Any,
    collections::{BTreeMap, HashSet},
    convert::Infallible,
};

use axum::{
    body::Body,
    extract::OriginalUri,
    http::{
        header::{self, HeaderValue},
        StatusCode,
    },
    response::IntoResponse,
    routing::{MethodRouter, Router},
    BoxError,
};
use dyn_clone::clone_box;
use http::{Request, Response};
use okapi::{openapi3, schemars::gen::SchemaGenerator};
use thiserror::Error;
#[cfg(feature = "grpc")]
use tonic::{
    body::Body as GrpcBody,
    server::NamedService,
    service::{Routes as GrpcRoutes, RoutesBuilder as GrpcRoutesBuilder},
};
#[cfg(feature = "grpc")]
use tower::Service;
use tower::{builder::ServiceBuilder, util::BoxCloneSyncService, ServiceExt};
use tower_http::{
    catch_panic::CatchPanicLayer,
    request_id::MakeRequestUuid,
    set_header::SetResponseHeaderLayer,
    trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer},
    LatencyUnit, ServiceBuilderExt,
};
use tracing::{debug, debug_span, info, info_span, warn};

use crate::{
    apidoc::{ApiDocBuilder, ApiDocError},
    auth::{
        AuthExtractor, AuthLayer, AuthProvider, AuthSetupError, BasicAuthExtractor,
        ConfigAuthProvider, HeaderAuthExtractor, NoOpAuthExtractor, NoOpAuthProvider,
    },
    config::AppConfig,
    errors,
    http_client::{HttpClientConfig, HttpClientError},
    layers::{
        ext::HandlerName, rate::RateLimitError, request_id::RecordRequestIdLayer,
        timeout::TimeoutError,
    },
    logging::span::CustomMakeSpan,
    metrics::{MetricsBuilder, MetricsError, MetricsState},
    state,
    tracing::TracingError,
    util::ResponseExtension,
};

/// Error type used in app builder.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AppBuilderError {
    /// API doc error.
    #[error(transparent)]
    ApiDoc(#[from] ApiDocError),
    /// Metrics error.
    #[error(transparent)]
    Metrics(#[from] MetricsError),
    /// Tracing error.
    #[error(transparent)]
    Tracing(#[from] TracingError),
    /// Duplicate handler name.
    #[error("Duplicate handler name: {0}")]
    DuplicateHandlerName(&'static str),
    /// HTTP client error.
    #[error("HTTP client error: {0}")]
    HttpClient(#[from] HttpClientError),
    /// HTTP client is absent from configuration.
    #[error("HTTP client is absent from configuration: {0}")]
    HttpClientAbsent(String),
    /// Auth framework error.
    #[error("Auth framework error: {0}")]
    Auth(#[from] AuthSetupError),
}

/// Builder for application routes.
#[derive(Debug)]
#[non_exhaustive]
pub struct AppBuilder {
    /// Authentication and authorization back-end.
    auth_provider: Box<dyn AuthProvider>,
    /// Authentication front-end.
    ///
    /// Handles protocol- and schema-specific message exchange.
    auth_extractor: Box<dyn AuthExtractor>,
    /// Application configuration.
    config: AppConfig,
    /// Metrics container object.
    metrics: Option<MetricsState>,
    /// Container of configured [`tonic`] GRPC services.
    #[cfg(feature = "grpc")]
    grpc_services: GrpcRoutesBuilder,
}

impl TryFrom<AppConfig> for AppBuilder {
    type Error = AppBuilderError;

    fn try_from(mut value: AppConfig) -> Result<Self, Self::Error> {
        let auth_provider = value.auth.make_provider()?;
        let auth_extractor = value.auth.extractor.make_extractor()?;
        Ok(Self {
            auth_provider,
            auth_extractor,
            metrics: value.metrics_state.take(),
            config: value,
            #[cfg(feature = "grpc")]
            grpc_services: GrpcRoutes::builder(),
        })
    }
}

impl Default for AppBuilder {
    fn default() -> Self {
        Self {
            auth_provider: Box::new(NoOpAuthProvider),
            auth_extractor: Box::new(NoOpAuthExtractor),
            config: AppConfig::default(),
            metrics: None,
            #[cfg(feature = "grpc")]
            grpc_services: GrpcRoutes::builder(),
        }
    }
}

impl AppBuilder {
    /// Create new builder with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create new builder with provided configuration.
    ///
    /// # Errors
    ///
    /// Returns `Err` if some part in builder initialization fails.
    pub fn from_config(cfg: &AppConfig) -> Result<Self, AppBuilderError> {
        cfg.clone().try_into()
    }

    /// Create [`tower`] auth layer for use in a specific handler.
    #[must_use]
    pub fn auth_layer<S>(&self, perms: &'static [&'static str]) -> AuthLayer<S> {
        AuthLayer::new(
            perms,
            clone_box(self.auth_provider.as_ref()),
            clone_box(self.auth_extractor.as_ref()),
        )
    }

    /// Set auth extractor directly.
    ///
    /// Normally you shouldn't use this method, relying instead on runtime configuration.
    pub fn with_auth_extractor(&mut self, extractor: impl AuthExtractor) -> &mut Self {
        self.auth_extractor = Box::new(extractor);
        self
    }

    /// Set auth provider directly.
    ///
    /// Normally you shouldn't use this method, relying instead on runtime configuration.
    pub fn with_auth_provider(&mut self, provider: impl AuthProvider) -> &mut Self {
        self.auth_provider = Box::new(provider);
        self
    }

    /// Configure application for HTTP Basic auth.
    ///
    /// Normally you shouldn't use this method, relying instead on runtime configuration.
    pub fn with_basic_auth(&mut self) -> &mut Self {
        self.auth_provider = Box::new(ConfigAuthProvider::from(self.config.auth.clone()));
        self.auth_extractor = Box::new(BasicAuthExtractor::new(None::<&str>));
        self
    }

    /// Configure application for authentication via HTTP headers.
    ///
    /// Normally you shouldn't use this method, relying instead on runtime configuration.
    pub fn with_header_auth(&mut self) -> &mut Self {
        self.auth_provider = Box::new(ConfigAuthProvider::from(self.config.auth.clone()));
        self.auth_extractor = Box::new(HeaderAuthExtractor::new(None::<&str>, None::<&str>));
        self
    }

    /// Set used API doc builder.
    ///
    /// The builder must be configured prior to passing it to this method. This enables OpenAPI
    /// spec generation, and an (optional) RapiDoc UI.
    ///
    /// Alternatively, you can include API doc configuration in [`AppConfig::api_doc`] section.
    pub fn with_api_doc(&mut self, api_doc: ApiDocBuilder) -> &mut Self {
        self.config.api_doc = Some(api_doc);
        self
    }

    /// Add state to be used in handlers using [`axum::extract::State`].
    pub fn with_state<S>(&mut self, state: S) -> &mut Self
    where
        S: Clone + Send + 'static,
    {
        // TODO: maybe make state registry non-global? dubious.
        state::put(state);
        self
    }

    /// Set used metrics builder.
    ///
    /// The builder must be configured prior to passing it to this method. This enables gathering
    /// of handler execution metrics, as well as an exporter HTTP endpoint.
    ///
    /// Alternatively, you can include metrics configuration in [`AppConfig::metrics`] section.
    pub fn with_metrics_config(&mut self, metrics: MetricsBuilder) -> &mut Self {
        self.config.metrics = Some(metrics);
        self
    }

    /// Configure metrics builder.
    ///
    /// This gets the builder from configuration, and then passes it to the provided callback
    /// function for additional customization.
    #[must_use]
    pub fn with_configured_metrics_config<F>(mut self, modifier: F) -> Self
    where
        F: FnOnce(Option<MetricsBuilder>) -> Option<MetricsBuilder>,
    {
        self.config.metrics = modifier(self.config.metrics);
        self
    }

    /// Configure API doc builder.
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

    /// Get metrics state object.
    ///
    /// Creates a new state object on first call.
    ///
    /// # Errors
    ///
    /// Returns `Err` if metrics registry or provider could not be initialized.
    pub fn metrics(&self) -> Option<&MetricsState> {
        self.metrics.as_ref()
    }

    /// Build top-level Axum router.
    ///
    /// # Errors
    ///
    /// Returns `Err` if some part of application setup did not succeed, or when there are
    /// conflicting handlers defined in application code.
    pub fn build(mut self) -> Result<Router, AppBuilderError> {
        let _build_span = debug_span!("build_app").entered();
        let mut rtr = Router::new();

        let metrics_state = self.metrics().cloned();
        if let (Some(m_state), Some(m_cfg)) = (&metrics_state, &self.config.metrics) {
            rtr = rtr.merge(m_cfg.build_router(m_state));
        }

        // Add probes and management mode API.
        rtr = rtr.merge(self.config.probes.build_router(
            clone_box(self.auth_provider.as_ref()),
            clone_box(self.auth_extractor.as_ref()),
        ));

        // A set to ensure uniqueness of handler names.
        let mut handler_names = HashSet::new();
        let mut grouped: BTreeMap<&str, Vec<&dyn HandlerExt>> = BTreeMap::new();
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

        // Register handlers.
        for (path, handlers) in grouped {
            if let Some(method_rtr) = self.register_path(path, handlers) {
                rtr = rtr.route(path, method_rtr.handle_error(error_handler));
            }
        }

        // Register GRPC services.
        #[cfg(feature = "grpc")]
        {
            // TODO: filter GRPC handlers on request content-type.
            rtr = rtr.merge(self.grpc_services.clone().routes().into_axum_router());
        }

        // Add RapiDoc and/or OpenAPI specification generator if enabled.
        if let Some(ref mut api_doc) = self.config.api_doc {
            let disabled = self
                .config
                .handlers
                .iter()
                .filter(|(_, v)| v.disabled)
                .map(|(k, _)| k.clone());
            api_doc.set_disabled_handlers(disabled);
            api_doc.set_app_defaults(
                self.config.app_name.as_deref(),
                self.config.app_version.as_deref(),
            );
            let auth = self.auth_extractor.security_schemes();
            rtr = rtr.merge(api_doc.build_router(auth)?);
        }

        // TODO: allow customizing fallback handler.
        rtr = rtr.fallback(fallback_handler);

        // Wrap router in global layers.
        let final_rtr = self.wrap_global_layers(rtr, metrics_state);
        info!("finished building application");
        Ok(final_rtr)
    }

    /// Build and return configured [`reqwest`] HTTP client with distributed tracing support.
    ///
    /// # Errors
    ///
    /// Returns `Err` if metrics registry or HTTP client could not be initialized.
    pub async fn http_client(
        &self,
        name: impl AsRef<str>,
    ) -> Result<reqwest_middleware::ClientWithMiddleware, AppBuilderError> {
        let name = name.as_ref();
        match self.config.http_clients.get(name).cloned() {
            Some(mut cfg) => {
                if let Some(app_name) = &self.config.app_name {
                    cfg.with_app_name(app_name);
                }
                if let Some(app_version) = &self.config.app_version {
                    cfg.with_app_version(app_version);
                }
                let metrics = self.metrics().map(|m| m.client_metrics(name));
                cfg.to_client(metrics).await.map_err(Into::into)
            }
            None => Err(AppBuilderError::HttpClientAbsent(name.to_string())),
        }
    }

    /// Same as [`Self::http_client`], but returns default client if there is no configuration
    /// available.
    ///
    /// # Errors
    ///
    /// Returns `Err` if metrics registry or HTTP client could not be initialized.
    pub async fn http_client_or_default(
        &self,
        name: impl AsRef<str>,
    ) -> Result<reqwest_middleware::ClientWithMiddleware, AppBuilderError> {
        let name = name.as_ref();
        match self.http_client(name).await {
            Ok(client) => Ok(client),
            Err(AppBuilderError::HttpClientAbsent(_)) => {
                let mut cfg = HttpClientConfig::default();
                if let Some(app_name) = &self.config.app_name {
                    cfg.with_app_name(app_name);
                }
                if let Some(app_version) = &self.config.app_version {
                    cfg.with_app_version(app_version);
                }
                let metrics = self.metrics().map(|m| m.client_metrics(name));
                cfg.to_client(metrics).await.map_err(Into::into)
            }
            Err(err) => Err(err),
        }
    }

    /// Wrap router in global [`tower`] layers.
    fn wrap_global_layers(&self, rtr: Router, metrics: Option<MetricsState>) -> Router {
        // [`tower`] layers that are executed for any request.
        let tracing_config = self.config.tracing.as_ref();
        let include_headers = tracing_config.is_some_and(|t| t.include_headers());
        let request_level =
            tracing_config.map_or(tracing::Level::DEBUG, |t| t.request_level().into());
        let response_level =
            tracing_config.map_or(tracing::Level::INFO, |t| t.response_level().into());
        let mut sensitive_headers = vec![header::AUTHORIZATION];
        sensitive_headers.append(&mut self.auth_extractor.sensitive_headers());
        let global_layers = ServiceBuilder::new()
            .set_x_request_id(MakeRequestUuid)
            .layer(RecordRequestIdLayer::new())
            .sensitive_headers(sensitive_headers)
            .layer(
                // TODO: factor out tracing for GRPC.
                TraceLayer::new_for_http()
                    .make_span_with(CustomMakeSpan::new().include_headers(include_headers))
                    .on_request(DefaultOnRequest::new().level(request_level))
                    .on_response(
                        DefaultOnResponse::new()
                            .level(response_level)
                            .include_headers(include_headers)
                            .latency_unit(LatencyUnit::Micros),
                    ),
            )
            .option_layer(metrics)
            .map_request(crate::logging::span::register_request)
            .propagate_x_request_id()
            .layer(SetResponseHeaderLayer::if_not_present(
                header::SERVER,
                self.server_header(),
            ))
            .layer(CatchPanicLayer::custom(panic_handler));
        // TODO: DefaultBodyLimit (configurable).
        rtr.layer(global_layers)
    }

    /// Register all handlers for a given path in [`MethodRouter`].
    ///
    /// Returns [`None`] if all handlers for a path are disabled.
    #[must_use]
    fn register_path(
        &self,
        path: &str,
        handlers: Vec<&dyn HandlerExt>,
    ) -> Option<MethodRouter<(), BoxError>> {
        let _register_span = info_span!("register_path", path).entered();
        let mut path_has_handlers = false;
        let mut method_rtr = MethodRouter::new();
        for handler in handlers {
            let name = handler.name();
            let _span = info_span!("register_handler", name, method = ?handler.method()).entered();
            if let Some(cfg) = self.config.handlers.get(name) {
                if cfg.disabled {
                    info!("skipping disabled handler");
                    continue;
                }
            }
            method_rtr = self.register_handler(method_rtr, handler);
            path_has_handlers = true;
            info!("handler registered");
        }
        path_has_handlers.then_some(method_rtr)
    }

    /// Register a handler in [`MethodRouter`].
    fn register_handler(
        &self,
        method_rtr: MethodRouter<(), BoxError>,
        handler: &dyn HandlerExt,
    ) -> MethodRouter<(), BoxError> {
        let service = self.handler_service(handler);
        match handler.method() {
            http::Method::GET => method_rtr.get_service(service),
            http::Method::HEAD => method_rtr.head_service(service),
            http::Method::POST => method_rtr.post_service(service),
            http::Method::PUT => method_rtr.put_service(service),
            http::Method::DELETE => method_rtr.delete_service(service),
            http::Method::OPTIONS => method_rtr.options_service(service),
            http::Method::TRACE => method_rtr.trace_service(service),
            http::Method::PATCH => method_rtr.patch_service(service),
            other => panic!("Unsupported HTTP method: {other}"),
        }
    }

    /// Convert a [`HandlerExt`] structure into a [`tower`] layered service.
    #[must_use]
    fn handler_service(
        &self,
        handler: &dyn HandlerExt,
    ) -> BoxCloneSyncService<Request<Body>, Response<Body>, BoxError> {
        let name = handler.name();
        let _span = info_span!("handler_service", name, method = ?handler.method()).entered();
        let service_cfg = self.config.handlers.get(name);
        // TODO: default catch-all CORS config?
        let cors_layer =
            service_cfg.and_then(|cfg| match cfg.cors.as_ref().map(|c| c.make_layer()) {
                None => None,
                Some(Ok(layer)) => Some(layer.allow_methods(handler.method())),
                Some(Err(err)) => {
                    warn!(error = %err, "Unable to build CORS layer");
                    None
                }
            });
        ServiceBuilder::new()
            .layer(BoxCloneSyncService::layer())
            .layer(ResponseExtension(HandlerName::new(name)))
            // Authentication layer.
            .option_layer(match handler.no_auth() {
                true => None,
                false => Some(self.auth_layer(handler.permissions())),
            })
            // Buffer layer.
            .option_layer(
                service_cfg.and_then(|cfg| cfg.buffer.as_ref())
                    .map(|lcfg| lcfg.make_layer()),
            )
            // Rate limiting layer.
            .option_layer(
                service_cfg.and_then(|cfg| cfg.rate_limit.as_ref())
                    .map(|rcfg| rcfg.make_layer()),
            )
            // CORS layer.
            .option_layer(cors_layer)
            // Timeout layer.
            .option_layer(service_cfg.map(|cfg| cfg.timeout.clone()).unwrap_or_default().make_layer())
            .service(handler.service().map_err(|err| err.into()))
    }

    /// Generate a value to be used in HTTP Server header.
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

    /// Embed [`tonic`] GRPC service into Axum application.
    #[cfg(feature = "grpc")]
    pub fn with_grpc_service<S>(&mut self, svc: S) -> &mut Self
    where
        S: Service<Request<GrpcBody>, Error = Infallible>
            + NamedService
            + Clone
            + Send
            + Sync
            + 'static,
        S::Response: IntoResponse,
        S::Future: Send + 'static,
    {
        self.grpc_services.add_service(svc);
        self
    }
}

/// Error handler for uxum-specific error types.
pub(crate) async fn error_handler(err: BoxError) -> Response<Body> {
    if let Some(rate_err) = err.downcast_ref::<RateLimitError>().cloned() {
        return rate_err.into_response();
    }
    if let Some(timeo_err) = err.downcast_ref::<TimeoutError>().cloned() {
        return timeo_err.into_response();
    }
    problemdetails::new(StatusCode::INTERNAL_SERVER_ERROR)
        .with_type(errors::TAG_UXUM_ERROR)
        .with_title(err.to_string())
        .into_response()
}

/// Error handler for when no handler is found by application router.
pub(crate) async fn fallback_handler(OriginalUri(uri): OriginalUri) -> Response<Body> {
    problemdetails::new(StatusCode::NOT_FOUND)
        .with_type(errors::TAG_UXUM_NOT_FOUND)
        .with_title("Resource not found")
        .with_value("uri", uri.to_string())
        .into_response()
}

/// Catch panics inside handlers and convert them into responses.
///
/// Used in [`CatchPanicLayer`].
fn panic_handler(err: Box<dyn Any + Send + 'static>) -> Response<Body> {
    let details = if let Some(s) = err.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = err.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "Unknown panic format".to_string()
    };
    problemdetails::new(StatusCode::INTERNAL_SERVER_ERROR)
        .with_type(errors::TAG_UXUM_PANIC)
        .with_title("Encountered panic in handler")
        .with_detail(details)
        .into_response()
}

/// Application API method handler object trait.
///
/// Using [`crate::handler`] macro will generate a unique unit struct type implementing this trait,
/// and register it using [`inventory::submit!`].
pub trait HandlerExt: Sync {
    /// Get handler name.
    ///
    /// Must be unique, otherwise app initialization will panic.
    fn name(&self) -> &'static str;
    /// Get URL path to run this handler.
    ///
    /// Uses [`axum::extract::Path`] format for embedded path parameters.
    fn path(&self) -> &'static str;
    /// Get URL path to run this handler, reformatted for OpenAPI specification.
    fn spec_path(&self) -> &'static str;
    /// Get HTTP method to run this handler.
    fn method(&self) -> http::Method;
    /// Get required permissions, if any.
    fn permissions(&self) -> &'static [&'static str];
    /// Skip authentication for this handler.
    fn no_auth(&self) -> bool;
    /// Return handler function packaged as a [`tower`] service.
    fn service(&self) -> BoxCloneSyncService<Request<Body>, Response<Body>, Infallible>;
    /// Generate OpenAPI specification object for handler.
    fn openapi_spec(&self, gen: &mut SchemaGenerator) -> openapi3::Operation;
}

// All handlers are registered at this point.
//
// This happens magically before `main()` is run.
// For more info see documentation on [`inventory`] crate.
inventory::collect!(&'static dyn HandlerExt);
