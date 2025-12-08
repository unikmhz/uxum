use std::{str::FromStr, time::Instant};

use http::Extensions;
use opentelemetry::{global, propagation::Injector};
use reqwest::{
    Request, Response,
    header::{HeaderName, HeaderValue},
};
use reqwest_middleware::{Error, Middleware, Next, Result};
use tracing::{Instrument, Level, Span, error, field::Empty, span};
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Export current distributed tracing info into a new request.
fn inject_otel_context(mut req: Request) -> Request {
    global::get_text_map_propagator(|prop| {
        let context = Span::current().context();
        prop.inject_context(&context, &mut RequestInjector::new(&mut req))
    });
    req
}

/// Adaptor to facilitate injection of distributed tracing information via HTTP request headers.
struct RequestInjector<'a> {
    request: &'a mut Request,
}

impl<'a> RequestInjector<'a> {
    /// Create new request injector for a particular request.
    fn new(request: &'a mut Request) -> Self {
        Self { request }
    }

    fn set_header(&mut self, key: &str, value: String) {
        let header_name = match HeaderName::from_str(key) {
            Ok(header) => header,
            Err(err) => {
                error!(err = %err, "Unable to format header name for trace propagation");
                return;
            }
        };
        let header_value = match HeaderValue::try_from(&value) {
            Ok(value) => value,
            Err(err) => {
                error!(err = %err, "Unable to format header value for trace propagation");
                return;
            }
        };
        let _ = self.request.headers_mut().insert(header_name, header_value);
    }
}

impl<'a> Injector for RequestInjector<'a> {
    fn set(&mut self, key: &str, value: String) {
        self.set_header(key, value);
    }
}

/// Disables propagation of OpenTelemetry distributed tracing information via HTTP headers if
/// inserted into a request as an extension.
#[derive(Clone, Debug)]
pub struct DisableOtelPropagation;

/// Middleware to log and trace outgoing HTTP requests.
#[derive(Clone, Debug, Default)]
pub struct TracingMiddleware;

impl TracingMiddleware {
    /// Create new middleware instance.
    pub fn new() -> Self {
        Default::default()
    }
}

#[async_trait::async_trait]
impl Middleware for TracingMiddleware {
    async fn handle(&self, req: Request, ext: &mut Extensions, next: Next<'_>) -> Result<Response> {
        let start_time = Instant::now();
        let request_span = {
            let method = req.method();
            let url = req.url();
            let scheme = url.scheme();
            let host = url.host_str().unwrap_or("");
            let host_port = url.port_or_known_default().unwrap_or(0) as i64;
            let otel_name = format!("{} {}", &method, url.path());
            let header_default = &::http::HeaderValue::from_static("");
            let user_agent = format!(
                "{:?}",
                req.headers().get("user-agent").unwrap_or(header_default)
            )
            .replace('"', "");
            // TODO: customize level
            span! {
                Level::INFO,
                "HTTP request",
                http.request.method = %method,
                url.scheme = %scheme,
                server.address = %host,
                server.port = %host_port,
                user_agent.original = %user_agent,
                otel.kind = "client",
                otel.name = %otel_name,
                otel.status_code = Empty,
                http.response.status_code = Empty,
                error.message = Empty,
                error.cause_chain = Empty,
                elapsed = Empty,
            }
        };
        let outcome_future = async {
            let req = if ext.get::<DisableOtelPropagation>().is_none() {
                inject_otel_context(req)
            } else {
                req
            };
            let outcome = next.run(req, ext).await;
            match &outcome {
                Ok(res) => on_request_success(&request_span, res),
                Err(err) => on_request_failure(&request_span, err),
            }
            //default_on_request_end(span, outcome);
            request_span.record("elapsed", start_time.elapsed().as_secs_f64());
            outcome
        };
        // TODO: remove clone
        outcome_future.instrument(request_span.clone()).await
    }
}

#[inline]
fn on_request_success(span: &Span, resp: &Response) {
    let status = resp.status().as_u16();
    span.record("http.response.status_code", status);
    if !matches!(status, 100..=399) {
        span.record("otel.status_code", "ERROR");
    }
}

#[inline]
fn on_request_failure(span: &Span, err: &Error) {
    let error_message = err.to_string();
    let error_cause_chain = format!("{:?}", err);
    span.record("otel.status_code", "ERROR");
    span.record("error.message", error_message.as_str());
    span.record("error.cause_chain", error_cause_chain.as_str());
    if let Error::Reqwest(e) = err {
        if let Some(status) = e.status() {
            span.record("http.response.status_code", status.as_u16());
        }
    }
}
