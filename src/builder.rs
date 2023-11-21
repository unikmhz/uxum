use std::{collections::HashMap, time::Duration};

use axum::{
    routing::{MethodRouter, Router},
    Extension,
};
use hyper::server::conn::AddrIncoming;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{
    trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
    LatencyUnit,
};

use crate::{HandlerName, ServerBuilderError};

///
pub struct AppBuilder;

impl AppBuilder {
    ///
    pub fn build() -> Router {
        let mut grouped: HashMap<&str, Vec<&'static dyn HandlerExt>> = HashMap::new();
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

/// TCP keepalive configuration.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
struct TcpKeepaliveConfig {
    /// Duration to remain idle before sending TCP keepalive probes. TCP keepalive is disabled if
    /// value is not provided.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    idle: Option<Duration>,
    /// Duration between two successive TCP keepalive retransmissions, if acknowledgement
    /// to the previous keepalive transmission is not received.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    interval: Option<Duration>,
    /// Number of retransmissions to be carried out before declaring that remote end is not
    /// available.
    retries: Option<u32>,
}

// TODO: TcpKeepaliveConfig runtime API

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct Http1Config {
    /// Enable HTTP/1 protocol.
    #[serde(default = "Http1Config::default_enabled")]
    enabled: bool,
    ///
    #[serde(default)]
    half_close: bool,
    ///
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    header_read_timeout: Option<Duration>,
    ///
    #[serde(default = "Http1Config::default_keepalive")]
    keepalive: bool,
    ///
    // TODO: humansize
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_buf_size: Option<usize>,
    ///
    #[serde(default, skip_serializing_if = "Option::is_none")]
    writev: Option<bool>,
}

impl Default for Http1Config {
    fn default() -> Self {
        Self {
            enabled: Self::default_enabled(),
            half_close: false,
            header_read_timeout: None,
            keepalive: Self::default_keepalive(),
            max_buf_size: None,
            writev: None,
        }
    }
}

impl Http1Config {
    ///
    fn default_enabled() -> bool {
        true
    }

    ///
    fn default_keepalive() -> bool {
        true
    }

    // TODO: Http1Config runtime API
}

///
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
struct Http2KeepaliveConfig {
    ///
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    interval: Option<Duration>,
    ///
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    timeout: Option<Duration>,
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct Http2Config {
    /// Enable HTTP/2 protocol.
    #[serde(default = "Http2Config::default_enabled")]
    enabled: bool,
    ///
    #[serde(default)]
    adaptive_window: bool,
    ///
    #[serde(default)]
    connect_protocol: bool,
    ///
    // TODO: humansize
    #[serde(default, skip_serializing_if = "Option::is_none")]
    initial_connection_window: Option<u32>,
    ///
    #[serde(default, skip_serializing_if = "Option::is_none")]
    initial_stream_window: Option<u32>,
    ///
    #[serde(default)]
    keepalive: Http2KeepaliveConfig,
    ///
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_concurrent_streams: Option<u32>,
}

impl Default for Http2Config {
    fn default() -> Self {
        Self {
            enabled: Self::default_enabled(),
            adaptive_window: false,
            connect_protocol: false,
            initial_connection_window: None,
            initial_stream_window: None,
            keepalive: Default::default(),
            max_concurrent_streams: None,
        }
    }
}

impl Http2Config {
    ///
    fn default_enabled() -> bool {
        true
    }

    // TODO: Http2Config runtime API
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ServerBuilder {
    /// Host/address and port to listen on.
    #[serde(default = "ServerBuilder::default_listen")]
    listen: String,
    /// Set TCP_NODELAY socket options for accepted connections.
    #[serde(default = "ServerBuilder::default_tcp_nodelay")]
    tcp_nodelay: bool,
    /// Sleep on accept errors.
    #[serde(default)]
    sleep_on_accept_errors: bool,
    /// TCP keepalive socket options.
    #[serde(default)]
    tcp_keepalive: TcpKeepaliveConfig,
    ///
    #[serde(default)]
    http1: Http1Config,
    ///
    #[serde(default)]
    http2: Http2Config,
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self {
            listen: Self::default_listen(),
            tcp_nodelay: Self::default_tcp_nodelay(),
            sleep_on_accept_errors: false,
            tcp_keepalive: Default::default(),
            http1: Default::default(),
            http2: Default::default(),
        }
    }
}

impl ServerBuilder {
    fn default_listen() -> String {
        "localhost:8080".into()
    }

    fn default_tcp_nodelay() -> bool {
        true
    }

    ///
    pub fn new() -> Self {
        Default::default()
    }

    ///
    pub async fn build(self) -> Result<hyper::server::Builder<AddrIncoming>, ServerBuilderError> {
        // TODO: low-level options for TcpListener construction
        let listener = TcpListener::bind(&self.listen)
            .await
            .map_err(ServerBuilderError::ListenerBind)?;
        let mut stream =
            AddrIncoming::from_listener(listener).map_err(ServerBuilderError::ListenerLocalAddr)?;
        stream
            .set_nodelay(self.tcp_nodelay)
            .set_keepalive(self.tcp_keepalive.idle)
            .set_keepalive_interval(self.tcp_keepalive.interval)
            .set_keepalive_retries(self.tcp_keepalive.retries)
            .set_sleep_on_errors(self.sleep_on_accept_errors);
        let mut builder = axum::Server::builder(stream);
        // TODO: check for NoProtocolsEnabled
        if self.http1.enabled {
            builder = builder
                .http1_half_close(self.http1.half_close)
                .http1_keepalive(self.http1.keepalive);
            if let Some(timeout) = self.http1.header_read_timeout {
                builder = builder.http1_header_read_timeout(timeout);
            }
            if let Some(bufsz) = self.http1.max_buf_size {
                builder = builder.http1_max_buf_size(bufsz);
            }
            if let Some(writev) = self.http1.writev {
                builder = builder.http1_writev(writev);
            }
        } else {
            if !self.http2.enabled {
                return Err(ServerBuilderError::NoProtocolsEnabled);
            }
            builder = builder.http2_only(true);
        }
        if self.http2.enabled {
            builder = builder
                .http2_adaptive_window(self.http2.adaptive_window)
                .http2_initial_connection_window_size(self.http2.initial_connection_window)
                .http2_initial_stream_window_size(self.http2.initial_stream_window)
                .http2_keep_alive_interval(self.http2.keepalive.interval)
                .http2_max_concurrent_streams(self.http2.max_concurrent_streams);
            if self.http2.connect_protocol {
                builder = builder.http2_enable_connect_protocol();
            }
            if let Some(timeout) = self.http2.keepalive.timeout {
                builder = builder.http2_keep_alive_timeout(timeout);
            }
        } else {
            builder = builder.http1_only(true);
        }
        Ok(builder)
    }
}
