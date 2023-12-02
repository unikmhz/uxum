use std::collections::BTreeMap;

use axum::{
    body::Body,
    error_handling::HandleErrorLayer,
    http::StatusCode,
    response::IntoResponse,
    routing::{MethodRouter, Router},
    Extension,
};
use hyper::{Request, Response};
use okapi::openapi3;
use tower::{builder::ServiceBuilder, util::BoxCloneService, BoxError, Service};
use tower_http::{
    trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
    LatencyUnit,
};

use crate::{AppConfig, HandlerConfig, HandlerName};

/// Builder for application routes.
#[derive(Debug, Default)]
pub struct AppBuilder {
    /// Application configuration.
    config: AppConfig,
}

impl From<AppConfig> for AppBuilder {
    fn from(value: AppConfig) -> Self {
        Self { config: value }
    }
}

impl AppBuilder {
    /// Create new builder with default configuration.
    pub fn new() -> Self {
        Default::default()
    }

    /// Build top-level Axum router.
    pub fn build(self) -> Router {
        let mut grouped: BTreeMap<&str, Vec<&'static dyn HandlerExt>> = BTreeMap::new();
        let mut rtr = Router::new();
        for handler in inventory::iter::<&'static dyn HandlerExt> {
            grouped
                .entry(handler.path())
                .and_modify(|handlers| handlers.push(*handler))
                .or_insert_with(|| vec![*handler]);
        }
        for (path, handlers) in grouped.into_iter() {
            let mut mrtr: MethodRouter<(), Body, BoxError> = MethodRouter::new();
            for handler in handlers {
                let name = handler.name();
                if let Some(cfg) = self.config.handlers.get(name) {
                    if cfg.disabled {
                        // TODO: log
                        continue;
                    }
                }
                mrtr = handler.register_method(mrtr, self.config.handlers.get(name));
            }
            rtr = rtr.route(path, mrtr.layer(HandleErrorLayer::new(error_handler)));
        }

        let global_layers = ServiceBuilder::new().layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().include_headers(true))
                .on_request(DefaultOnRequest::new().level(tracing::Level::DEBUG))
                .on_response(
                    DefaultOnResponse::new()
                        .level(tracing::Level::INFO)
                        .latency_unit(LatencyUnit::Micros),
                ),
        );

        rtr.layer(global_layers)
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
    T: Send + 'static,
    U: 'static,
{
    ServiceBuilder::new()
        .boxed_clone()
        .layer(Extension(HandlerName::new(hext.name())))
        .option_layer(
            conf.and_then(|cfg| cfg.buffer.as_ref())
                .map(|lcfg| lcfg.make_layer()),
        )
        // .option_layer(
        //     conf.and_then(|cfg| cfg.rate_limit.as_ref())
        //         .and_then(|rcfg| rcfg.make_layer()),
        // )
        // TODO: cb
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
