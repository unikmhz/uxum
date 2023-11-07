use std::time::Duration;

use hyper::server::conn::AddrIncoming;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::ServerBuilderError;

///
pub struct RouteBuilder {
}

impl RouteBuilder {
    ///
    pub fn new() -> Self {
        Self {}
    }
}

/// TCP keepalive configuration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct KeepaliveConfig {
    /// Duration to remain idle before sending TCP keepalive probes.
    #[serde(with = "humantime_serde")]
    idle: Duration,
    /// Duration between two successive TCP keepalive retransmissions, if acknowledgement
    /// to the previous keepalive transmission is not received.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "humantime_serde")]
    interval: Option<Duration>,
    /// Number of retransmissions to be carried out before declaring that remote end is not
    /// available.
    retries: Option<u32>,
}

// TODO: KeepaliveConfig runtime API

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct Http1Config {
    /// Enable HTTP/1 protocol.
    #[serde(default = "Http1Config::default_enabled")]
    enabled: bool,
    ///
    #[serde(default)]
    half_close: bool,
}

impl Default for Http1Config {
    fn default() -> Self {
        Self {
            enabled: true,
            half_close: false,
        }
    }
}

impl Http1Config {
    ///
    fn default_enabled() -> bool {
        true
    }

    // TODO: Http1Config runtime API
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct Http2Config {
    /// Enable HTTP/2 protocol.
    #[serde(default = "Http2Config::default_enabled")]
    enabled: bool,
}

impl Default for Http2Config {
    fn default() -> Self {
        Self {
            enabled: true,
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
    listen: String,
    /// Set TCP_NODELAY socket options for accepted connections.
    #[serde(default = "ServerBuilder::default_tcp_nodelay")]
    tcp_nodelay: bool,
    /// Sleep on accept errors.
    #[serde(default)]
    sleep_on_accept_errors: bool,
    /// TCP keepalive socket options. Disabled if not provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tcp_keepalive: Option<KeepaliveConfig>,
    ///
    #[serde(default)]
    http1: Option<Http1Config>,
    ///
    #[serde(default)]
    http2: Option<Http2Config>,
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self {
            listen: "localhost:8080".into(),
            tcp_nodelay: true,
            sleep_on_accept_errors: false,
            tcp_keepalive: None,
            http1: None,
            http2: None,
        }
    }
}

impl ServerBuilder {
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
        let mut stream = AddrIncoming::from_listener(listener)
            .map_err(ServerBuilderError::ListenerLocalAddr)?;
        stream
            .set_nodelay(self.tcp_nodelay)
            .set_sleep_on_errors(self.sleep_on_accept_errors);
        if let Some(keepalive) = self.tcp_keepalive {
            stream
                .set_keepalive(Some(keepalive.idle))
                .set_keepalive_interval(keepalive.interval)
                .set_keepalive_retries(keepalive.retries);
        }
        let mut builder = axum::Server::builder(stream);
        // TODO: check for NoProtocolsEnabled
        if let Some(http1) = self.http1 {
            if http1.enabled {
                builder = builder.http1_half_close(http1.half_close);
            } else {
                if let Some(http2) = &self.http2 {
                    if !http2.enabled {
                        return Err(ServerBuilderError::NoProtocolsEnabled);
                    }
                }
                builder = builder.http2_only(true);
            }
        }
        if let Some(http2) = self.http2 {
            if http2.enabled {
            } else {
                builder = builder.http1_only(true);
            }
        }
        Ok(builder)
    }
}
