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
use okapi::openapi3;
use tower::{builder::ServiceBuilder, util::BoxCloneService, BoxError, Service};
use tower_http::{
    set_header::SetResponseHeaderLayer,
    trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
    LatencyUnit,
};
use tracing::{debug, debug_span, info, info_span};

use crate::{ApiDocBuilder, AppConfig, HandlerConfig, HandlerName};

/// Builder for application routes.
#[derive(Debug, Default)]
pub struct AppBuilder {
    /// Application configuration.
    config: AppConfig,
    ///
    app_name: Option<String>,
    ///
    app_version: Option<String>,
    /// API docs configuration.
    api_doc: Option<ApiDocBuilder>,
}

impl From<AppConfig> for AppBuilder {
    fn from(value: AppConfig) -> Self {
        Self {
            config: value,
            app_name: None,
            app_version: None,
            api_doc: None,
        }
    }
}

impl AppBuilder {
    /// Create new builder with default configuration.
    pub fn new() -> Self {
        Default::default()
    }

    ///
    pub fn with_app_name(mut self, app_name: impl ToString) -> Self {
        self.app_name = Some(app_name.to_string());
        self
    }

    ///
    pub fn with_app_version(mut self, app_version: impl ToString) -> Self {
        self.app_version = Some(app_version.to_string());
        self
    }

    ///
    pub fn with_api_doc(mut self, api_doc: ApiDocBuilder) -> Self {
        self.api_doc = Some(api_doc);
        self
    }

    /// Build top-level Axum router.
    pub fn build(self) -> Router {
        let _span = debug_span!("build_app").entered();
        let mut grouped: BTreeMap<&str, Vec<&dyn HandlerExt>> = BTreeMap::new();
        let mut rtr = Router::new();
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
                info!("Handler registered");
            }
            rtr = rtr.route(path, mrtr.layer(HandleErrorLayer::new(error_handler)));
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

        let rtr = rtr.layer(global_layers);
        info!("Finished building application");
        rtr
    }

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
    fn method(&self) -> http::Method;
    fn register_method(
        &self,
        mrtr: MethodRouter<(), Body, BoxError>,
        cfg: Option<&HandlerConfig>,
    ) -> MethodRouter<(), Body, BoxError>;
    fn openapi_spec(&self) -> Option<openapi3::Operation> {
        None
    }
}

inventory::collect!(&'static dyn HandlerExt);
