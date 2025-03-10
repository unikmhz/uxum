//! Custom span generators for request tracing.

use axum::{body::Body, http::Request};
use opentelemetry::{propagation::Extractor, trace::TraceContextExt};
use tower_http::{request_id::RequestId, trace::MakeSpan};
use tracing::{field::Empty, Level, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;

const DEFAULT_MESSAGE_LEVEL: Level = Level::DEBUG;

/// Custom span creation for [`tower_http::trace::TraceLayer`].
#[derive(Debug, Clone)]
pub(crate) struct CustomMakeSpan {
    /// Verbosity level of created span.
    level: Level,
    /// Include HTTP request headers as span attributes.
    include_headers: bool,
}

impl Default for CustomMakeSpan {
    fn default() -> Self {
        Self {
            level: DEFAULT_MESSAGE_LEVEL,
            include_headers: false,
        }
    }
}

impl CustomMakeSpan {
    /// Create new span creator with default settings.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Set the [`Level`] used for the [tracing span].
    ///
    /// Defaults to [`Level::DEBUG`].
    ///
    /// [tracing span]: https://docs.rs/tracing/latest/tracing/#spans
    #[allow(dead_code)]
    pub(crate) fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// Include request headers on the [`Span`].
    ///
    /// By default headers are not included.
    pub(crate) fn include_headers(mut self, include_headers: bool) -> Self {
        self.include_headers = include_headers;
        self
    }
}

impl MakeSpan<Body> for CustomMakeSpan {
    fn make_span(&mut self, request: &Request<Body>) -> Span {
        // TODO: don't send trace/span IDs as redundant attributes in otel traces.
        let x_request_id = request
            .extensions()
            .get::<RequestId>()
            .and_then(|id| id.header_value().to_str().ok());
        // This ugly macro is needed, unfortunately, because `tracing::span!`
        // required the level argument to be static. Meaning we can't just pass
        // `self.level`.
        macro_rules! make_span {
            ($level:expr) => {
                if self.include_headers {
                    tracing::span!(
                        $level,
                        "request",
                        "otel.kind" = "server",
                        "trace_id" = Empty,
                        "span_id" = Empty,
                        "x_request_id" = x_request_id,
                        "http.request.method" = %request.method(),
                        "url.full" = %request.uri(),
                        "http.version" = ?request.version(),
                        "http.request.headers" = ?request.headers(),
                    )
                } else {
                    tracing::span!(
                        $level,
                        "request",
                        "otel.kind" = "server",
                        "trace_id" = Empty,
                        "span_id" = Empty,
                        "x_request_id" = x_request_id,
                        "http.request.method" = %request.method(),
                        "url.full" = %request.uri(),
                        "http.version" = ?request.version(),
                    )
                }
            }
        }

        match self.level {
            Level::ERROR => make_span!(Level::ERROR),
            Level::WARN => make_span!(Level::WARN),
            Level::INFO => make_span!(Level::INFO),
            Level::DEBUG => make_span!(Level::DEBUG),
            Level::TRACE => make_span!(Level::TRACE),
        }
    }
}

/// Helper for extracting headers from HTTP Requests. This is used for OpenTelemetry context
/// propagation over HTTP.
/// See [this](https://github.com/open-telemetry/opentelemetry-rust/blob/main/examples/tracing-http-propagator/README.md)
/// for example usage.
///
/// This is lifted verbatim from [`opentelemetry_http`] crate due to [`http`] crate version
/// incompatibilities.
///
/// [`opentelemetry_http`]: https://docs.rs/opentelemetry-http
// TODO: move back to standard extractor when everyone synchronizes their version of http crate.
pub(crate) struct HeaderExtractor<'a>(pub(crate) &'a http::HeaderMap);

impl Extractor for HeaderExtractor<'_> {
    /// Get a value for a key from the [`http::HeaderMap`].  If the value is not valid ASCII, returns None.
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }

    /// Collect all the keys from the [`http::HeaderMap`].
    fn keys(&self) -> Vec<&str> {
        self.0
            .keys()
            .map(|value| value.as_str())
            .collect::<Vec<_>>()
    }
}

pub(crate) fn register_request(req: Request<Body>) -> Request<Body> {
    // TODO: don't lookup trace/span IDs, use values pre-extracted by tracing-opentelemetry.
    // TODO: don't send trace/span IDs as redundant attributes in otel traces.
    let parent_context = opentelemetry::global::get_text_map_propagator(|prop| {
        prop.extract(&HeaderExtractor(req.headers()))
    });
    let span = Span::current();
    span.set_parent(parent_context);
    let trace_id = span.context().span().span_context().trace_id();
    let span_id = span.context().span().span_context().span_id();
    span.record("trace_id", trace_id.to_string());
    span.record("span_id", span_id.to_string());
    req
}
