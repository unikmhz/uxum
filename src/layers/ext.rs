//! Miscellaneous types used as request extensions.

use std::{
    borrow::{Borrow, BorrowMut},
    fmt,
    hash::Hash,
    net::SocketAddr,
    ops::{Deref, DerefMut},
    time::{Duration, Instant},
};

use tokio::time::Instant as TokioInstant;

/// Static handler name.
///
/// This gets attached as an extension to requests and responses for use mainly in middleware
/// layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct HandlerName(&'static str);

impl HandlerName {
    /// Construct new [`HandlerName`] from static string slice.
    #[must_use]
    pub fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// Get static string slice stored inside.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

impl AsRef<str> for HandlerName {
    fn as_ref(&self) -> &str {
        self.0
    }
}

impl Deref for HandlerName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl Borrow<str> for HandlerName {
    fn borrow(&self) -> &str {
        self.0
    }
}

impl fmt::Display for HandlerName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Cutoff time after which the request must be timed out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Deadline(Instant);

impl Deadline {
    /// Construct new [`Deadline`] with zero time left.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if deadline has passed.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.0 >= Instant::now()
    }

    /// Get remaining time.
    ///
    /// Returns [`None`] if deadline has passed.
    #[must_use]
    pub fn time_left(&self) -> Option<Duration> {
        Instant::now().checked_duration_since(self.0)
    }
}

impl Default for Deadline {
    fn default() -> Self {
        Self(Instant::now())
    }
}

impl From<Duration> for Deadline {
    fn from(value: Duration) -> Self {
        Self(Instant::now() + value)
    }
}

impl From<Instant> for Deadline {
    fn from(value: Instant) -> Self {
        Self(value)
    }
}

impl From<TokioInstant> for Deadline {
    fn from(value: TokioInstant) -> Self {
        Self(value.into_std())
    }
}

impl AsRef<Instant> for Deadline {
    fn as_ref(&self) -> &Instant {
        &self.0
    }
}

impl Deref for Deadline {
    type Target = Instant;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Deadline {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Borrow<Instant> for Deadline {
    fn borrow(&self) -> &Instant {
        &self.0
    }
}

impl BorrowMut<Instant> for Deadline {
    fn borrow_mut(&mut self) -> &mut Instant {
        &mut self.0
    }
}

impl fmt::Display for Deadline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}s left",
            self.time_left().unwrap_or_default().as_secs_f64()
        )
    }
}

/// Describes listener's protocol and its features.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListenerProtocol {
    /// Plain HTTP over TCP.
    Http,
    /// HTTP over TLS over TCP using static preconfigured certificates.
    HttpsTls,
    /// HTTP over TLS over TCP using SPIFFE-distributed certificates.
    #[cfg(feature = "spiffe")]
    HttpsSpiffe,
    /// QUIC over UDP using static preconfigured certificates.
    Quic,
}

impl ListenerProtocol {
    /// Get URL scheme formatted as OpenTelemetry attribute value.
    pub fn as_scheme(&self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::HttpsTls | Self::Quic => "https",
            #[cfg(feature = "spiffe")]
            Self::HttpsSpiffe => "https",
        }
    }

    /// Get transport protocol formatted as OpenTelemetry attribute value.
    pub fn as_transport(&self) -> &'static str {
        match self {
            Self::Quic => "udp",
            _ => "tcp",
        }
    }
}

/// This structure is registered as an extension for every request via a custom acceptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListenerInfo {
    /// Listener protocol.
    pub protocol: ListenerProtocol,
    /// Local listening address and port, if applicable.
    pub local_addr: SocketAddr,
}

impl ListenerInfo {
    /// Create listener info object for plain HTTP over TCP.
    pub fn new_http(local_addr: SocketAddr) -> Self {
        Self {
            protocol: ListenerProtocol::Http,
            local_addr,
        }
    }

    /// Create listener info object for HTTP over TLS over TCP.
    pub fn new_tls(local_addr: SocketAddr) -> Self {
        Self {
            protocol: ListenerProtocol::HttpsTls,
            local_addr,
        }
    }

    /// Create listener info object for HTTP over SPIFFE/TLS over TCP.
    #[cfg(feature = "spiffe")]
    pub fn new_spiffe(local_addr: SocketAddr) -> Self {
        Self {
            protocol: ListenerProtocol::HttpsSpiffe,
            local_addr,
        }
    }

    /// Create listener info object for QUIC over UDP.
    pub fn new_quic(local_addr: SocketAddr) -> Self {
        Self {
            protocol: ListenerProtocol::Quic,
            local_addr,
        }
    }
}
