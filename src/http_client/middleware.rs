//! HTTP client - middleware setup.

use http::{Extensions, HeaderValue};
use recloser::AsyncRecloser;
use reqwest::{Client, Request, Response};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Middleware, Next, Result};

use crate::{
    http_client::{CircuitBreakerMiddleware, MetricsMiddleware, TracingMiddleware},
    layers::{
        request_id::{CURRENT_REQUEST_ID, X_REQUEST_ID},
        timeout::{CURRENT_DEADLINE, X_TIMEOUT},
    },
    metrics::ClientMetricsState,
};

/// Middleware to propagate useful headers to chained requests.
struct HeaderPropagationMiddleware;

#[async_trait::async_trait]
impl Middleware for HeaderPropagationMiddleware {
    async fn handle(
        &self,
        mut req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> Result<Response> {
        let x_req_id = CURRENT_REQUEST_ID
            .try_with(|req_id| req_id.as_ref().map(|r| r.clone().into_header_value()))
            .ok();
        let x_timeout = CURRENT_DEADLINE
            .try_with(|deadline| {
                deadline
                    .and_then(|instant| instant.time_left())
                    .and_then(|duration| {
                        // TODO: proper ISO8601 duration formatting.
                        HeaderValue::from_str(&format!("PT{:.2}S", duration.as_secs_f64())).ok()
                    })
            })
            .ok();
        if let Some(Some(req_id)) = x_req_id {
            req.headers_mut().insert(X_REQUEST_ID, req_id);
        }
        if let Some(Some(timeout)) = x_timeout {
            req.headers_mut().insert(X_TIMEOUT, timeout);
        }
        next.run(req, extensions).await
    }
}

/// Wrap [`reqwest::Client`] with our custom middleware stack.
pub(crate) fn wrap_client(
    client: Client,
    metrics: Option<ClientMetricsState>,
    cb: Option<AsyncRecloser>,
) -> ClientWithMiddleware {
    let mut builder = ClientBuilder::new(client)
        .with(HeaderPropagationMiddleware)
        .with(TracingMiddleware::new());
    if let Some(metrics) = metrics {
        builder = builder.with(MetricsMiddleware::from(metrics));
    }
    if let Some(cb) = cb {
        builder = builder.with(CircuitBreakerMiddleware::from(cb));
    }
    builder.build()
}
