use std::{
    net::SocketAddr,
    num::{NonZeroU32, NonZeroUsize},
    time::Duration,
};

use axum_server::Handle;
use serde::{Deserialize, Serialize};
use socket2::SockRef;
use thiserror::Error;
use tokio::{
    net::{lookup_host, TcpSocket, ToSocketAddrs},
    task::JoinHandle,
};
use tracing::{debug, debug_span, error, info, Instrument};

use crate::{
    errors::IoError,
    signal::{SignalError, SignalStream},
};

/// Error type returned by server builder
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ServerBuilderError {
    /// Unable to parse endpoint address
    #[error("Unable to parse endpoint address: {0}")]
    AddressParse(IoError),
    /// Unable to resolve DNS name
    #[error("Unable to resolve DNS name: {0}")]
    Resolve(String),
    /// Unable to create socket
    #[error("Unable to create socket: {0}")]
    SocketCreate(IoError),
    /// Unable to bind socket to local address
    #[error("Unable to bind socket to local address {0}: {1}")]
    BindAddr(SocketAddr, IoError),
    /// Unable to listen on socket
    #[error("Unable to listen on socket {0}: {1}")]
    Listen(SocketAddr, IoError),
    /// Unable to perform conversion into std listener
    #[error("Unable to perform conversion into std listener: {0}")]
    ConvertListener(IoError),
    /// Unable to extract local address
    #[error("Unable to extract local address: {0}")]
    ListenerLocalAddr(hyper::Error),
    /// Unable to set SO_REUSEADDR
    #[error("Unable to set SO_REUSEADDR: {0}")]
    SetReuseAddr(IoError),
    /// Unable to set SO_RCVBUF
    #[error("Unable to set SO_RCVBUF: {0}")]
    SetRecvBuffer(IoError),
    /// Unable to set SO_SNDBUF
    #[error("Unable to set SO_SNDBUF: {0}")]
    SetSendBuffer(IoError),
    /// Unable to set SO_KEEPALIVE
    #[error("Unable to set SO_KEEPALIVE: {0}")]
    SetKeepAlive(IoError),
    /// Unable to set IP_TOS
    #[error("Unable to set IP_TOS: {0}")]
    SetIpTos(IoError),
    /// Unable to set TCP_MAXSEG
    #[error("Unable to set TCP_MAXSEG: {0}")]
    SetTcpMss(IoError),
    /// Unable to set TCP_NODELAY
    #[error("Unable to set TCP_NODELAY: {0}")]
    SetNoDelay(IoError),
    /// Neither HTTP/1 nor HTTP/2 are enabled
    #[error("Neither HTTP/1 nor HTTP/2 are enabled")]
    NoProtocolsEnabled,
    /// Signal handler error
    #[error(transparent)]
    SignalHandler(#[from] SignalError),
}

/// Builder for HTTP server
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct ServerBuilder {
    /// Host/address and port to listen on
    #[serde(default = "ServerBuilder::default_listen")]
    pub listen: String,
    /// Sleep on accept errors
    #[serde(default)]
    pub sleep_on_accept_errors: bool,
    /// Size of TCP receive buffer, in bytes
    #[serde(default)]
    pub recv_buffer: Option<NonZeroUsize>,
    /// Size of TCP send buffer, in bytes
    #[serde(default)]
    pub send_buffer: Option<NonZeroUsize>,
    /// IP-level socket configuration
    #[serde(default)]
    pub ip: IpConfig,
    /// TCP-level socket configuration
    #[serde(default)]
    pub tcp: TcpConfig,
    /// Configuration specific to HTTP/1 protocol
    #[serde(default)]
    pub http1: Http1Config,
    /// Configuration specific to HTTP/2 protocol
    #[serde(default)]
    pub http2: Http2Config,
    // TODO: TLS
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self {
            listen: Self::default_listen(),
            sleep_on_accept_errors: false,
            recv_buffer: None,
            send_buffer: None,
            ip: IpConfig::default(),
            tcp: TcpConfig::default(),
            http1: Http1Config::default(),
            http2: Http2Config::default(),
        }
    }
}

impl ServerBuilder {
    /// Default value for [`Self::listen`]
    #[must_use]
    #[inline]
    fn default_listen() -> String {
        "localhost:8080".into()
    }

    /// Create new server builder with default configuration
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build network server
    ///
    /// # Errors
    ///
    /// Returns `Err` if builder encounters an error while setting up a listening socket.
    pub async fn build(self) -> Result<axum_server::Server, ServerBuilderError> {
        let span = debug_span!("build_server");
        async move {
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
            if let Some(idle) = self.tcp.keepalive.idle {
                let mut tcp_keepalive = socket2::TcpKeepalive::new().with_time(idle);
                if let Some(interval) = self.tcp.keepalive.interval {
                    tcp_keepalive = tcp_keepalive.with_interval(interval);
                }
                if let Some(retries) = self.tcp.keepalive.retries {
                    tcp_keepalive = tcp_keepalive.with_retries(retries.get());
                }
                sref.set_tcp_keepalive(&tcp_keepalive)
                    .map_err(|err| ServerBuilderError::SetKeepAlive(err.into()))?;
            } else {
                sref.set_keepalive(false)
                    .map_err(|err| ServerBuilderError::SetKeepAlive(err.into()))?;
            }
            sock.bind(addr)
                .map_err(|err| ServerBuilderError::BindAddr(addr, err.into()))?;
            sock.set_nodelay(self.tcp.nodelay)
                .map_err(|err| ServerBuilderError::SetNoDelay(err.into()))?;
            let listener = sock
                .listen(self.tcp.backlog.get())
                .map_err(|err| ServerBuilderError::Listen(addr, err.into()))?
                .into_std()
                .map_err(|err| ServerBuilderError::ConvertListener(err.into()))?;
            // TODO: from_tcp_rustls for TLS
            let mut server = axum_server::from_tcp(listener);
            let builder = server.http_builder();

            {
                debug!("setting up HTTP/1");
                let mut http1 = builder.http1();
                http1
                    .half_close(self.http1.half_close)
                    .keep_alive(self.http1.keepalive);
                if let Some(timeout) = self.http1.header_read_timeout {
                    http1.header_read_timeout(timeout);
                }
                if let Some(bufsz) = self.http1.max_buf_size {
                    http1.max_buf_size(bufsz.get());
                }
                if let Some(writev) = self.http1.writev {
                    http1.writev(writev);
                }
            }
            {
                debug!("setting up HTTP/2");
                let mut http2 = builder.http2();
                http2
                    .adaptive_window(self.http2.adaptive_window)
                    .initial_connection_window_size(
                        self.http2.initial_connection_window.map(NonZeroU32::get),
                    )
                    .initial_stream_window_size(
                        self.http2.initial_stream_window.map(NonZeroU32::get),
                    )
                    .keep_alive_interval(self.http2.keepalive.interval)
                    .max_concurrent_streams(self.http2.max_concurrent_streams.map(NonZeroU32::get));
                if self.http2.connect_protocol {
                    http2.enable_connect_protocol();
                }
                if let Some(timeout) = self.http2.keepalive.timeout {
                    http2.keep_alive_timeout(timeout);
                }
            }

            info!("finished building server");
            Ok(server)
        }
        .instrument(span)
        .await
    }

    /// Launch a task that captures common UNIX signals
    ///
    /// This will gracefully shut down the server if signal type is appropriate.
    pub fn spawn_signal_handler(
        &self,
        handle: Handle,
    ) -> Result<JoinHandle<()>, ServerBuilderError> {
        let span = debug_span!("signal_handler");
        let mut sig = SignalStream::new()?;
        Ok(tokio::spawn(
            async move {
                loop {
                    match sig.next().await {
                        Ok(sig) if sig.is_shutdown() => {
                            info!("received {}, shutting down server", sig.name());
                            // FIXME: configure duration
                            handle.graceful_shutdown(Some(Duration::from_secs(5)));
                            break;
                        }
                        Ok(sig) => {
                            debug!("don't know what to do with signal {}, ignoring", sig.name());
                        }
                        Err(err) => {
                            error!("error in signal handler: {err}");
                        }
                    }
                }
            }
            .instrument(span),
        ))
    }
}

/// IP-level configuration
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct IpConfig {
    /// IP TOS value, as a number
    #[serde(default)]
    pub tos: Option<u32>,
}

/// TCP-level configuration
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct TcpConfig {
    /// Set TCP_NODELAY socket options for accepted connections
    #[serde(default = "crate::util::default_true")]
    pub nodelay: bool,
    /// Size of TCP backlog queue
    #[serde(default = "TcpConfig::default_backlog")]
    pub backlog: NonZeroU32,
    /// Size of TCP MSS, in bytes
    #[serde(default)]
    pub mss: Option<NonZeroU32>,
    /// TCP keep-alive socket options
    #[serde(default)]
    pub keepalive: TcpKeepaliveConfig,
}

impl Default for TcpConfig {
    fn default() -> Self {
        Self {
            nodelay: true,
            backlog: Self::default_backlog(),
            mss: None,
            keepalive: TcpKeepaliveConfig::default(),
        }
    }
}

impl TcpConfig {
    /// Default value for [`Self::backlog`]
    #[must_use]
    #[inline]
    #[allow(clippy::unwrap_used)]
    fn default_backlog() -> NonZeroU32 {
        // SAFETY: 1024 is always not a zero
        NonZeroU32::new(1024).unwrap()
    }
}

/// TCP keepalive configuration
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct TcpKeepaliveConfig {
    /// Duration to remain idle before sending TCP keepalive probes
    ///
    /// TCP keepalive is disabled if value is not provided.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub idle: Option<Duration>,
    /// Duration between two successive TCP keepalive retransmissions, if acknowledgement
    /// to the previous keepalive transmission is not received
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub interval: Option<Duration>,
    /// Number of retransmissions to be carried out before declaring that remote end is not
    /// available
    pub retries: Option<NonZeroU32>,
}

/// HTTP/1 protocol configuration
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Http1Config {
    /// Support half-closed HTTP/1 connections
    ///
    /// See [`hyper_util::server::conn::auto::Http1Builder::half_close`].
    #[serde(default)]
    pub half_close: bool,
    /// Maximum allowed time to wait for client to send HTTP header
    ///
    /// If this time is reached without a complete header present, the client connection is closed.
    ///
    /// See [`hyper_util::server::conn::auto::Http1Builder::header_read_timeout`].
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub header_read_timeout: Option<Duration>,
    /// Enable HTTP/1 keep-alive
    #[serde(default = "crate::util::default_true")]
    pub keepalive: bool,
    /// Set maximum per-connection buffer size
    ///
    /// Default is approx. 400KiB.
    ///
    /// See [`hyper_util::server::conn::auto::Http1Builder::max_buf_size`].
    // TODO: bytesize
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_buf_size: Option<NonZeroUsize>,
    /// Use vectored I/O when writing to network sockets
    ///
    /// See [`hyper_util::server::conn::auto::Http1Builder::writev`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writev: Option<bool>,
}

impl Default for Http1Config {
    fn default() -> Self {
        Self {
            half_close: false,
            header_read_timeout: None,
            keepalive: true,
            max_buf_size: None,
            writev: None,
        }
    }
}

/// HTTP/2 protocol configuration
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Http2Config {
    /// Enable HTTP/2 adaptive flow control
    #[serde(default)]
    pub adaptive_window: bool,
    /// Enable the [extended CONNECT protocol](https://datatracker.ietf.org/doc/html/rfc8441#section-4)
    #[serde(default)]
    pub connect_protocol: bool,
    /// Set the max connection-level flow control for HTTP/2
    // TODO: bytesize
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_connection_window: Option<NonZeroU32>,
    /// Set initial connection window for a stream
    ///
    /// Sets the [SETTINGS_INITIAL_WINDOW_SIZE](https://http2.github.io/http2-spec/#SETTINGS_INITIAL_WINDOW_SIZE)
    /// option for HTTP/2 connections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_stream_window: Option<NonZeroU32>,
    /// HTTP/2 keep-alive configuration
    #[serde(default)]
    pub keepalive: Http2KeepaliveConfig,
    /// Sets the [SETTINGS_MAX_CONCURRENT_STREAMS](https://httpwg.org/specs/rfc9113.html#n-stream-concurrency)
    /// option for HTTP/2 connections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrent_streams: Option<NonZeroU32>,
}

/// HTTP/2 keep-alive configuration
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Http2KeepaliveConfig {
    /// HTTP/2 keep-alive interval
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub interval: Option<Duration>,
    /// HTTP/2 keep-alive timeout
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub timeout: Option<Duration>,
}

/// Turn DNS name or address into a socket
///
/// # Errors
///
/// Returns an error if there was an error parsing or resolving a name, or later on
/// when creating a socket. Note that if a name resolves to multiple addresses then
/// all of them are tried in order.
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

/// Resolve DNS name or address into a list of socket address/port pairs
///
/// # Errors
///
/// Returns `Err` if there was an error parsing or resolving a name.
/// Note that not-found errors result in `Ok` with an empty iterator.
async fn resolve<O>(origin: &O) -> Result<impl Iterator<Item = SocketAddr> + '_, ServerBuilderError>
where
    O: ToSocketAddrs + ToString,
{
    // TODO: use hickory-resolver crate
    lookup_host(origin)
        .await
        .map_err(|err| ServerBuilderError::AddressParse(err.into()))
}

/// Create a socket of needed type, based on the type of passed in socked address
///
/// # Errors
///
/// Returns `Err` if there was a problem creating a socket or setting `SO_REUSEADDR`
/// socket option.
fn sock_create(addr: &SocketAddr) -> Result<TcpSocket, ServerBuilderError> {
    let socket = match addr {
        SocketAddr::V4(_) => TcpSocket::new_v4(),
        SocketAddr::V6(_) => TcpSocket::new_v6(),
    }
    .map_err(|err| ServerBuilderError::SocketCreate(err.into()))?;

    #[cfg(not(windows))]
    socket
        .set_reuseaddr(true)
        .map_err(|err| ServerBuilderError::SetReuseAddr(err.into()))?;

    Ok(socket)
}
