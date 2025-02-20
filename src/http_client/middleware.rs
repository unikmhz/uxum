//! HTTP client - middleware setup.

use std::time::Instant;

use http::{Extensions, HeaderValue};
use hyper::body::Body;
use opentelemetry::KeyValue;
use recloser::AsyncRecloser;
use reqwest::{Client, Request, Response};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Error, Middleware, Next, Result};
use reqwest_tracing::{
    default_on_request_end, reqwest_otel_span, ReqwestOtelSpanBackend, TracingMiddleware,
};
use tracing::{field::Empty, Span};

use crate::{
    layers::{
        request_id::{CURRENT_REQUEST_ID, X_REQUEST_ID},
        timeout::{CURRENT_DEADLINE, X_TIMEOUT},
    },
    metrics::ClientMetricsState,
};

/// Custom delegate to create OpenTelemetry spans for distributed tracing.
struct ReqwestSpanBackend;

impl ReqwestOtelSpanBackend for ReqwestSpanBackend {
    #[allow(unexpected_cfgs)]
    fn on_request_start(req: &Request, ext: &mut Extensions) -> Span {
        ext.insert(Instant::now());
        let name = format!("{} {}", req.method(), req.url().path());
        reqwest_otel_span!(name = name, req, elapsed = Empty)
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

/// HTTP client metrics middleware.
struct MetricsMiddleware(ClientMetricsState);

#[async_trait::async_trait]
impl Middleware for MetricsMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> Result<Response> {
        let metrics = self.0.metrics();
        let name = self.0.name();
        let start = Instant::now();
        let method = req.method().clone();
        let scheme = req.url().scheme().to_string();
        let request_size = match req.body() {
            Some(b) => b.size_hint().upper().unwrap_or_default(),
            None => 0,
        };
        let active_labels = [
            KeyValue::new("http.client", name.to_string()),
            KeyValue::new("http.request.method", method.to_string()),
            KeyValue::new("url.scheme", scheme.clone()),
        ];
        metrics.requests_active.add(1, &active_labels);
        let resp = next.run(req, extensions).await;
        metrics.requests_active.add(-1, &active_labels);
        let duration = start.elapsed().as_secs_f64();
        let status = match &resp {
            Ok(r) => r.status().as_u16().to_string(),
            Err(_) => String::new(),
        };
        let response_size = match &resp {
            Ok(r) => r.content_length().unwrap_or_default(),
            Err(_) => 0,
        };
        let labels = [
            KeyValue::new("http.client", name.to_string()),
            KeyValue::new("http.request.method", method.to_string()),
            KeyValue::new("url.scheme", scheme.clone()),
        ];
        if let Err(Error::Middleware(ref err)) = resp {
            if err.is::<CircuitBreakerRejection>() {
                metrics.requests_rejected.add(1, &labels);
            } else {
                metrics.requests_errored.add(1, &labels);
            }
        }
        let mut labels = labels.into_iter();
        // SAFETY: labels array is guaranteed to be of size 3.
        let labels = [
            labels.next().unwrap(),
            labels.next().unwrap(),
            labels.next().unwrap(),
            KeyValue::new("http.response.status_code", status),
        ];
        metrics.requests_total.add(1, &labels);
        metrics.request_duration.record(duration, &labels);
        metrics.request_body_size.record(request_size, &labels);
        metrics.response_body_size.record(response_size, &labels);
        resp
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
        .with(TracingMiddleware::<ReqwestSpanBackend>::new());
    if let Some(metrics) = metrics {
        builder = builder.with(MetricsMiddleware(metrics));
    }
    if let Some(cb) = cb {
        builder = builder.with(CircuitBreakerMiddleware(cb));
    }
    builder.build()
}
