use std::collections::BTreeMap;

use axum::{
    routing::{MethodRouter, Router},
    Extension,
};
use tower::ServiceBuilder;
use tower_http::{
    trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
    LatencyUnit,
};

use crate::{AppConfig, HandlerName};

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
            let mut mrtr = MethodRouter::new();
            for handler in handlers {
                // TODO: add layers from config
                let layers =
                    ServiceBuilder::new().layer(Extension(HandlerName::new(handler.name())));
                mrtr = handler.register_method(mrtr.layer(layers));
            }
            rtr = rtr.route(path, mrtr);
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

pub trait HandlerExt: Sync {
    fn name(&self) -> &'static str;
    fn path(&self) -> &'static str;
    fn method(&self) -> http::Method;
    fn register_method(&self, mrtr: MethodRouter) -> MethodRouter;
}

inventory::collect!(&'static dyn HandlerExt);
