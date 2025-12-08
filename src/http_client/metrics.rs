use std::time::Instant;

use http::Extensions;
use hyper::body::Body;
use opentelemetry::KeyValue;
use reqwest::{Request, Response};
use reqwest_middleware::{Error, Middleware, Next, Result};

use crate::{http_client::cb::CircuitBreakerRejection, metrics::ClientMetricsState};

/// HTTP client metrics middleware.
#[derive(Clone, Debug)]
pub struct MetricsMiddleware(ClientMetricsState);

impl From<ClientMetricsState> for MetricsMiddleware {
    fn from(value: ClientMetricsState) -> Self {
        Self(value)
    }
}

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
        // TODO: insert Instant in separate layer
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
