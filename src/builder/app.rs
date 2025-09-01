//! [`axum`] application builder.

use std::{
    any::Any,
    borrow::Borrow,
    collections::{BTreeMap, HashSet},
    convert::Infallible,
};

use axum::{
    body::Body,
    http::{
        header::{self, HeaderValue},
        StatusCode,
    },
    response::IntoResponse,
    routing::{MethodRouter, Router},
    BoxError,
};
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
        AuthExtractor, AuthLayer, AuthProvider, BasicAuthExtractor, ConfigAuthProvider,
        HeaderAuthExtractor, NoOpAuthExtractor, NoOpAuthProvider,
    },
    config::AppConfig,
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
}

/// Builder for application routes.
#[derive(Debug)]
#[non_exhaustive]
pub struct AppBuilder<AuthProv = NoOpAuthProvider, AuthExt = NoOpAuthExtractor> {
    /// Authentication and authorization back-end.
    auth_provider: AuthProv,
    /// Authentication front-end.
    ///
    /// Handles protocol- and schema-specific message exchange.
    auth_extractor: AuthExt,
    /// Application configuration.
    config: AppConfig,
    /// Metrics container object.
    metrics: Option<MetricsState>,
    /// Container of configured [`tonic`] GRPC services.
    #[cfg(feature = "grpc")]
    grpc_services: GrpcRoutesBuilder,
}

impl From<AppConfig> for AppBuilder {
    fn from(value: AppConfig) -> Self {
        Self {
            auth_provider: NoOpAuthProvider,
            auth_extractor: NoOpAuthExtractor,
            config: value,
            metrics: None,
            #[cfg(feature = "grpc")]
            grpc_services: GrpcRoutes::builder(),
        }
    }
}

impl Default for AppBuilder<NoOpAuthProvider, NoOpAuthExtractor> {
    fn default() -> Self {
        Self {
            auth_provider: NoOpAuthProvider,
            auth_extractor: NoOpAuthExtractor,
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
    #[must_use]
    pub fn from_config(cfg: &AppConfig) -> Self {
        cfg.clone().into()
    }
}

impl<AuthProv, AuthExt> AppBuilder<AuthProv, AuthExt>
where
    AuthProv: AuthProvider + Sync + 'static,
    AuthExt: AuthExtractor + Sync + 'static,
    AuthExt::User: Borrow<AuthProv::User>,
    AuthExt::AuthTokens: Borrow<AuthProv::AuthTokens>,
{
    /// Enable HTTP Basic authentication using built-in user and role databases.
    #[must_use]
    pub fn with_basic_auth(self) -> AppBuilder<ConfigAuthProvider, BasicAuthExtractor> {
        AppBuilder {
            auth_provider: self.config.auth.clone().into(),
            auth_extractor: BasicAuthExtractor::default(),
            config: self.config,
            metrics: self.metrics,
            #[cfg(feature = "grpc")]
            grpc_services: self.grpc_services,
        }
    }

    /// Enable header authentication using built-in user and role databases.
    #[must_use]
    pub fn with_header_auth(self) -> AppBuilder<ConfigAuthProvider, HeaderAuthExtractor> {
        AppBuilder {
            auth_provider: self.config.auth.clone().into(),
            auth_extractor: HeaderAuthExtractor::default(),
            config: self.config,
            metrics: self.metrics,
            #[cfg(feature = "grpc")]
            grpc_services: self.grpc_services,
        }
    }

    /// Set custom authentication extractor (front-end).
    #[must_use]
    pub fn with_auth_extractor<E: AuthExtractor>(
        self,
        auth_extractor: E,
    ) -> AppBuilder<AuthProv, E> {
        AppBuilder {
            auth_provider: self.auth_provider,
            auth_extractor,
            config: self.config,
            metrics: self.metrics,
            #[cfg(feature = "grpc")]
            grpc_services: self.grpc_services,
        }
    }

    /// Set custom authentication provider (back-end).
    #[must_use]
    pub fn with_auth_provider<P: AuthProvider>(self, auth_provider: P) -> AppBuilder<P, AuthExt> {
        AppBuilder {
            auth_provider,
            auth_extractor: self.auth_extractor,
            config: self.config,
            metrics: self.metrics,
            #[cfg(feature = "grpc")]
            grpc_services: self.grpc_services,
        }
    }

    /// Create [`tower`] auth layer for use in a specific handler.
    #[must_use]
    pub fn auth_layer<S>(&self, perms: &'static [&'static str]) -> AuthLayer<S, AuthProv, AuthExt> {
        AuthLayer::new(
            perms,
            self.auth_provider.clone(),
            self.auth_extractor.clone(),
        )
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
    pub fn with_metrics(&mut self, metrics: MetricsBuilder) -> &mut Self {
        self.config.metrics = metrics;
        self
    }

    /// Configure metrics builder.
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
    pub fn metrics(&mut self) -> Result<&MetricsState, AppBuilderError> {
        // TODO: make metrics optional.
        match self.metrics {
            Some(ref metrics) => Ok(metrics),
            None => {
                let otel_res = self.config.otel_resource();
                let metrics = self.config.metrics.build_state(otel_res.clone())?;
                self.metrics = Some(metrics);
                // SAFETY: Some() is guaranteed, as we assigned it before.
                Ok(self.metrics.as_ref().unwrap())
            }
        }
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

        // Build metrics subsystem.
        let metrics_state = self.metrics()?.clone();
        if self.config.metrics.is_enabled() {
            rtr = rtr.merge(metrics_state.build_router());
        }

        // Add probes and management mode API.
        rtr = rtr.merge(
            self.config
                .probes
                .build_router(self.auth_provider.clone(), self.auth_extractor.clone()),
        );

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
        &mut self,
        name: impl AsRef<str>,
    ) -> Result<reqwest_middleware::ClientWithMiddleware, AppBuilderError> {
        let metrics = self.metrics()?.client_metrics(name);
        match self.config.http_clients.get_mut(metrics.name()) {
            Some(cfg) => {
                if let Some(app_name) = &self.config.app_name {
                    cfg.with_app_name(app_name);
                }
                if let Some(app_version) = &self.config.app_version {
                    cfg.with_app_version(app_version);
                }
                cfg.to_client(Some(metrics)).await.map_err(Into::into)
            }
            None => Err(AppBuilderError::HttpClientAbsent(
                metrics.name().to_string(),
            )),
        }
    }

    /// Same as [`Self::http_client`], but returns default client if there is no configuration
    /// available.
    ///
    /// # Errors
    ///
    /// Returns `Err` if metrics registry or HTTP client could not be initialized.
    pub async fn http_client_or_default(
        &mut self,
        name: impl AsRef<str>,
    ) -> Result<reqwest_middleware::ClientWithMiddleware, AppBuilderError> {
        let metrics = self.metrics()?.client_metrics(name);
        match self.http_client(metrics.name()).await {
            Ok(client) => Ok(client),
            Err(AppBuilderError::HttpClientAbsent(_)) => {
                let mut cfg = HttpClientConfig::default();
                if let Some(app_name) = &self.config.app_name {
                    cfg.with_app_name(app_name);
                }
                if let Some(app_version) = &self.config.app_version {
                    cfg.with_app_version(app_version);
                }
                cfg.to_client(Some(metrics)).await.map_err(Into::into)
            }
            Err(err) => Err(err),
        }
    }

    /// Wrap router in global [`tower`] layers.
    fn wrap_global_layers(&self, rtr: Router, metrics: MetricsState) -> Router {
        // [`tower`] layers that are executed for any request.
        let tracing_config = self.config.tracing.as_ref();
        let include_headers = tracing_config.map_or(false, |t| t.include_headers());
        let request_level =
            tracing_config.map_or(tracing::Level::DEBUG, |t| t.request_level().into());
        let response_level =
            tracing_config.map_or(tracing::Level::INFO, |t| t.response_level().into());
        let global_layers = ServiceBuilder::new()
            .set_x_request_id(MakeRequestUuid)
            .layer(RecordRequestIdLayer::new())
            .sensitive_headers([header::AUTHORIZATION])
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
            .layer(metrics)
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

impl<AuthProv> AppBuilder<AuthProv, BasicAuthExtractor> {
    /// Set realm used for HTTP authentication challenge.
    ///
    /// Default value is "auth".
    #[must_use]
    pub fn with_auth_realm(mut self, realm: impl AsRef<str>) -> Self {
        self.auth_extractor.set_realm(realm);
        self
    }
}

impl<AuthProv> AppBuilder<AuthProv, HeaderAuthExtractor> {
    /// Set user ID header name for use in authentication.
    ///
    /// Default value is "X-API-Name".
    #[must_use]
    pub fn with_user_header(mut self, name: impl AsRef<str>) -> Self {
        self.auth_extractor.set_user_header(name);
        self
    }

    /// Set authenticating token header name for use in authentication.
    ///
    /// Default value is "X-API-Key".
    #[must_use]
    pub fn with_tokens_header(mut self, name: impl AsRef<str>) -> Self {
        self.auth_extractor.set_tokens_header(name);
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
        .with_type("tag:uxum.github.io,2024:error")
        .with_title(err.to_string())
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
        .with_type("tag:uxum.github.io,2024:panic")
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
