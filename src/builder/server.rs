use std::{
    net::SocketAddr,
    num::{NonZeroU32, NonZeroUsize},
    time::Duration,
};

use hyper::server::conn::AddrIncoming;
use serde::{Deserialize, Serialize};
use socket2::SockRef;
use thiserror::Error;
use tokio::net::{lookup_host, TcpSocket, ToSocketAddrs};
use tracing::{debug, debug_span, error, info};

use crate::errors::IoError;

/// Error type returned by server builder.
#[derive(Debug, Error)]
pub enum ServerBuilderError {
    #[error("Unable to parse endpoint address: {0}")]
    AddressParse(IoError),
    #[error("Unable to resolve DNS name: {0}")]
    Resolve(String),
    #[error("Unable to create socket: {0}")]
    SocketCreate(IoError),
    #[error("Unable to set SO_REUSEADDR: {0}")]
    ReuseAddr(IoError),
    #[error("Unable to bind socket to local address {0}: {1}")]
    BindAddr(SocketAddr, IoError),
    #[error("Unable to listen on socket {0}: {1}")]
    Listen(SocketAddr, IoError),
    #[error("Unable to extract local address: {0}")]
    ListenerLocalAddr(hyper::Error),
    #[error("Unable to set SO_RCVBUF: {0}")]
    SetRecvBuffer(IoError),
    #[error("Unable to set SO_SNDBUF: {0}")]
    SetSendBuffer(IoError),
    #[error("Unable to set IP_TOS: {0}")]
    SetIpTos(IoError),
    #[error("Unable to set TCP_MAXSEG: {0}")]
    SetTcpMss(IoError),
    #[error("Neither HTTP/1 nor HTTP/2 are enabled")]
    NoProtocolsEnabled,
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ServerBuilder {
    /// Host/address and port to listen on.
    #[serde(default = "ServerBuilder::default_listen")]
    listen: String,
    /// Sleep on accept errors.
    #[serde(default)]
    sleep_on_accept_errors: bool,
    ///
    #[serde(default)]
    recv_buffer: Option<NonZeroUsize>,
    ///
    #[serde(default)]
    send_buffer: Option<NonZeroUsize>,
    ///
    #[serde(default)]
    ip: IpConfig,
    ///
    #[serde(default)]
    tcp: TcpConfig,
    ///
    #[serde(default)]
    http1: Http1Config,
    ///
    #[serde(default)]
    http2: Http2Config,
    // TODO: TLS
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self {
            listen: Self::default_listen(),
            sleep_on_accept_errors: false,
            recv_buffer: None,
            send_buffer: None,
            ip: Default::default(),
            tcp: Default::default(),
            http1: Default::default(),
            http2: Default::default(),
        }
    }
}

impl ServerBuilder {
    fn default_listen() -> String {
        "localhost:8080".into()
    }

    ///
    pub fn new() -> Self {
        Default::default()
    }

    /// Build network server.
    pub async fn build(self) -> Result<hyper::server::Builder<AddrIncoming>, ServerBuilderError> {
        let _span = debug_span!("build_server").entered();
        let (sock, addr) = socket(&self.listen).await?;
        let sref = SockRef::from(&sock);
        if let Some(sz) = self.recv_buffer {
            sref.set_recv_buffer_size(sz.get())
                .map_err(|err| ServerBuilderError::SetRecvBuffer(err.into()))?;
        }
        if let Some(sz) = self.send_buffer {
            sref.set_send_buffer_size(sz.get())
                .map_err(|err| ServerBuilderError::SetSendBuffer(err.into()))?;
        }
        if let Some(tos) = self.ip.tos {
            sref.set_tos(tos)
                .map_err(|err| ServerBuilderError::SetIpTos(err.into()))?;
        }
        if let Some(mss) = self.tcp.mss {
            sref.set_mss(mss.get())
                .map_err(|err| ServerBuilderError::SetTcpMss(err.into()))?;
        }
        sock.bind(addr)
            .map_err(|err| ServerBuilderError::BindAddr(addr, err.into()))?;
        let listener = sock
            .listen(self.tcp.backlog.get())
            .map_err(|err| ServerBuilderError::Listen(addr, err.into()))?;
        let mut stream =
            AddrIncoming::from_listener(listener).map_err(ServerBuilderError::ListenerLocalAddr)?;
        stream
            .set_nodelay(self.tcp.nodelay)
            .set_keepalive(self.tcp.keepalive.idle)
            .set_keepalive_interval(self.tcp.keepalive.interval)
            .set_keepalive_retries(self.tcp.keepalive.retries.map(NonZeroU32::get))
            .set_sleep_on_errors(self.sleep_on_accept_errors);
        let mut builder = hyper::Server::builder(stream);
        // TODO: check for NoProtocolsEnabled
        if self.http1.enabled {
            debug!("Setting up HTTP/1");
            builder = builder
                .http1_half_close(self.http1.half_close)
                .http1_keepalive(self.http1.keepalive);
            if let Some(timeout) = self.http1.header_read_timeout {
                builder = builder.http1_header_read_timeout(timeout);
            }
            if let Some(bufsz) = self.http1.max_buf_size {
                builder = builder.http1_max_buf_size(bufsz.get());
            }
            if let Some(writev) = self.http1.writev {
                builder = builder.http1_writev(writev);
            }
        } else {
            if !self.http2.enabled {
                error!("No protocols enabled");
                return Err(ServerBuilderError::NoProtocolsEnabled);
            }
            builder = builder.http2_only(true);
        }
        if self.http2.enabled {
            debug!("Setting up HTTP/2");
            builder = builder
                .http2_adaptive_window(self.http2.adaptive_window)
                .http2_initial_connection_window_size(
                    self.http2.initial_connection_window.map(NonZeroU32::get),
                )
                .http2_initial_stream_window_size(
                    self.http2.initial_stream_window.map(NonZeroU32::get),
                )
                .http2_keep_alive_interval(self.http2.keepalive.interval)
                .http2_max_concurrent_streams(
                    self.http2.max_concurrent_streams.map(NonZeroU32::get),
                );
            if self.http2.connect_protocol {
                builder = builder.http2_enable_connect_protocol();
            }
            if let Some(timeout) = self.http2.keepalive.timeout {
                builder = builder.http2_keep_alive_timeout(timeout);
            }
        } else {
            builder = builder.http1_only(true);
        }
        info!("Finished building server");
        Ok(builder)
    }
}

///
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct IpConfig {
    ///
    #[serde(default)]
    tos: Option<u32>,
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TcpConfig {
    /// Set TCP_NODELAY socket options for accepted connections.
    #[serde(default = "crate::util::default_true")]
    nodelay: bool,
    ///
    #[serde(default = "TcpConfig::default_tcp_backlog")]
    backlog: NonZeroU32,
    ///
    #[serde(default)]
    mss: Option<NonZeroU32>,
    /// TCP keepalive socket options.
    #[serde(default)]
    keepalive: TcpKeepaliveConfig,
}

impl Default for TcpConfig {
    fn default() -> Self {
        Self {
            nodelay: true,
            backlog: Self::default_tcp_backlog(),
            mss: None,
            keepalive: Default::default(),
        }
    }
}

impl TcpConfig {
    fn default_tcp_backlog() -> NonZeroU32 {
        NonZeroU32::new(1024).unwrap()
    }
}

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
    retries: Option<NonZeroU32>,
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct Http1Config {
    /// Enable HTTP/1 protocol.
    #[serde(default = "crate::util::default_true")]
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
    #[serde(default = "crate::util::default_true")]
    keepalive: bool,
    ///
    // TODO: humansize
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_buf_size: Option<NonZeroUsize>,
    ///
    #[serde(default, skip_serializing_if = "Option::is_none")]
    writev: Option<bool>,
}

impl Default for Http1Config {
    fn default() -> Self {
        Self {
            enabled: true,
            half_close: false,
            header_read_timeout: None,
            keepalive: true,
            max_buf_size: None,
            writev: None,
        }
    }
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct Http2Config {
    /// Enable HTTP/2 protocol.
    #[serde(default = "crate::util::default_true")]
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
    initial_connection_window: Option<NonZeroU32>,
    ///
    #[serde(default, skip_serializing_if = "Option::is_none")]
    initial_stream_window: Option<NonZeroU32>,
    ///
    #[serde(default)]
    keepalive: Http2KeepaliveConfig,
    ///
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_concurrent_streams: Option<NonZeroU32>,
}

impl Default for Http2Config {
    fn default() -> Self {
        Self {
            enabled: true,
            adaptive_window: false,
            connect_protocol: false,
            initial_connection_window: None,
            initial_stream_window: None,
            keepalive: Default::default(),
            max_concurrent_streams: None,
        }
    }
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

async fn socket<O>(origin: O) -> Result<(TcpSocket, SocketAddr), ServerBuilderError>
where
    O: ToSocketAddrs + ToString,
{
    let mut ret_err = None;
    let ret = resolve(&origin)
        .await?
        .find_map(|addr| match sock_create(&addr) {
            Ok(sock) => Some((sock, addr)),
            Err(err) => {
                ret_err = Some(err);
                None
            }
        });
    match ret {
        Some(pair) => Ok(pair),
        None => match ret_err {
            Some(err) => Err(err),
            None => Err(ServerBuilderError::Resolve(origin.to_string())),
        },
    }
}

async fn resolve<O>(origin: &O) -> Result<impl Iterator<Item = SocketAddr> + '_, ServerBuilderError>
where
    O: ToSocketAddrs + ToString,
{
    // TODO: use hickory-resolver crate
    lookup_host(origin)
        .await
        .map_err(|err| ServerBuilderError::AddressParse(err.into()))
}

fn sock_create(addr: &SocketAddr) -> Result<TcpSocket, ServerBuilderError> {
    let socket = match addr {
        SocketAddr::V4(_) => TcpSocket::new_v4(),
        SocketAddr::V6(_) => TcpSocket::new_v6(),
    }
    .map_err(|err| ServerBuilderError::SocketCreate(err.into()))?;

    #[cfg(not(windows))]
    socket
        .set_reuseaddr(true)
        .map_err(|err| ServerBuilderError::ReuseAddr(err.into()))?;

    Ok(socket)
}
