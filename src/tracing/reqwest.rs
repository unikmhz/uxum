use std::time::Instant;

use http::Extensions;
use reqwest::{Client, Request, Response};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Result};
use reqwest_tracing::{
    default_on_request_end, reqwest_otel_span, ReqwestOtelSpanBackend, TracingMiddleware,
};
use tracing::{field::Empty, Span};

struct ReqwestSpanBackend;

impl ReqwestOtelSpanBackend for ReqwestSpanBackend {
    fn on_request_start(req: &Request, ext: &mut Extensions) -> Span {
        ext.insert(Instant::now());
        reqwest_otel_span!(name = "http-request", req, elapsed = Empty)
    }

    fn on_request_end(span: &Span, outcome: &Result<Response>, ext: &mut Extensions) {
        default_on_request_end(span, outcome);
        if let Some(inst) = ext.get::<Instant>() {
            span.record("elapsed", inst.elapsed().as_secs_f64());
        }
    }
}

pub(crate) fn wrap_client(client: Client) -> ClientWithMiddleware {
    ClientBuilder::new(client)
        .with(TracingMiddleware::<ReqwestSpanBackend>::new())
        .build()
}
