use std::time::Instant;

use http::{Extensions, HeaderValue};
use recloser::AsyncRecloser;
use reqwest::{Client, Request, Response};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Error, Middleware, Next, Result};
use reqwest_tracing::{
    default_on_request_end, reqwest_otel_span, ReqwestOtelSpanBackend, TracingMiddleware,
};
use tracing::{field::Empty, Span};

use crate::layers::{
    request_id::{CURRENT_REQUEST_ID, X_REQUEST_ID},
    timeout::{CURRENT_DEADLINE, X_TIMEOUT},
};

/// Custom delegate to create OpenTelemetry spans for distributed tracing.
struct ReqwestSpanBackend;

impl ReqwestOtelSpanBackend for ReqwestSpanBackend {
    fn on_request_start(req: &Request, ext: &mut Extensions) -> Span {
        ext.insert(Instant::now());
        // TODO: maybe method + path in name?
        reqwest_otel_span!(name = "http-request", req, elapsed = Empty)
    }

    fn on_request_end(span: &Span, outcome: &Result<Response>, ext: &mut Extensions) {
        default_on_request_end(span, outcome);
        if let Some(inst) = ext.get::<Instant>() {
            span.record("elapsed", inst.elapsed().as_secs_f64());
        }
    }
}

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
                        // TODO: proper ISO8601 duration formatting
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

/// Circuit breaker middleware.
struct CircuitBreakerMiddleware(AsyncRecloser);

/// Circuit breaker rejection error.
#[derive(Clone, Debug, thiserror::Error)]
#[error("Request rejected, circuit breaker is open")]
struct CircuitBreakerRejection;

#[async_trait::async_trait]
impl Middleware for CircuitBreakerMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> Result<Response> {
        match self.0.call(next.run(req, extensions)).await {
            Ok(resp) => Ok(resp),
            Err(recloser::Error::Rejected) => Err(Error::middleware(CircuitBreakerRejection)),
            Err(recloser::Error::Inner(err)) => Err(err),
        }
    }
}

/// Wrap [`reqwest::Client`] with our custom middleware stack.
pub(crate) fn wrap_client(client: Client, cb: Option<AsyncRecloser>) -> ClientWithMiddleware {
    let mut builder = ClientBuilder::new(client)
        .with(HeaderPropagationMiddleware)
        .with(TracingMiddleware::<ReqwestSpanBackend>::new());
    if let Some(cb) = cb {
        builder = builder.with(CircuitBreakerMiddleware(cb));
    }
    builder.build()
}
