use std::{collections::BTreeMap, num::NonZeroU32, path::Path, str::FromStr, time::Duration};

use reqwest::{
    header::{HeaderName, HeaderValue},
    ClientBuilder, Identity,
};
use reqwest_middleware::ClientWithMiddleware;
use serde::{Deserialize, Serialize};
use tokio::{fs::OpenOptions, io::AsyncReadExt};

use crate::{http_client::errors::HttpClientError, tracing::reqwest::wrap_client};

/// HTTP client configuration
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct HttpClientConfig {
    /// Path to PEM-formatted file containing a private key and at least one client certificate
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "client_key",
        alias = "identity"
    )]
    pub client_cert: Option<Box<Path>>,
    /// Set a timeout for only the connect phase of a client
    ///
    /// Default is `None`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub connect_timeout: Option<Duration>,
    /// Enables a read timeout
    ///
    /// The timeout applies to each read operation, and resets after a successful read.
    /// This is more appropriate for detecting stalled connections when the size isn’t known beforehand.
    ///
    /// Default is no timeout.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub read_timeout: Option<Duration>,
    /// Enables a total request timeout
    ///
    /// The timeout is applied from when the request starts connecting until the response body has
    /// finished. Also considered a total deadline.
    ///
    /// Default is no timeout.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub request_timeout: Option<Duration>,
    /// Set an optional timeout for idle sockets being kept-alive
    ///
    /// Set to `None` to disable timeout.
    ///
    /// Default is 90 seconds.
    #[serde(
        default = "HttpClientConfig::default_pool_idle_timeout",
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub pool_idle_timeout: Option<Duration>,
    /// Sets the maximum idle connection per host allowed in the pool
    #[serde(default = "HttpClientConfig::default_pool_max_idle_per_host")]
    pub pool_max_idle_per_host: usize,
    /// Set whether connections should emit verbose logs
    ///
    /// Enabling this option will emit [`log`] messages at the `TRACE` level for read and write
    /// operations on connections.
    ///
    /// [`log`]: https://crates.io/crates/log
    #[serde(default)]
    pub verbose: bool,
    /// Sets the default headers for every request
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra_headers: BTreeMap<String, String>,
    /// Set a redirect policy for this client
    ///
    /// Default will follow redirects up to a maximum of 10.
    #[serde(default)]
    pub redirect: HttpClientRedirectPolicy,
    /// Enable or disable automatic setting of the `Referer` header
    ///
    /// Default is `true`.
    #[serde(default = "crate::util::default_true")]
    pub referer: bool,
    /// TCP-level configuration
    #[serde(default)]
    pub tcp: HttpClientTcpConfig,
    /// HTTP/2 protocol configuration
    #[serde(default)]
    pub http2: HttpClientHttp2Config,
    /// Short application name
    #[serde(skip)]
    app_name: Option<String>,
    /// Application version
    #[serde(skip)]
    app_version: Option<String>,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            client_cert: None,
            connect_timeout: None,
            read_timeout: None,
            request_timeout: None,
            pool_idle_timeout: Self::default_pool_idle_timeout(),
            pool_max_idle_per_host: Self::default_pool_max_idle_per_host(),
            verbose: false,
            extra_headers: BTreeMap::new(),
            redirect: HttpClientRedirectPolicy::default(),
            referer: true,
            tcp: HttpClientTcpConfig::default(),
            http2: HttpClientHttp2Config::default(),
            app_name: None,
            app_version: None,
        }
    }
}

impl HttpClientConfig {
    /// Default value for [`Self::pool_idle_timeout`]
    #[must_use]
    #[inline]
    fn default_pool_idle_timeout() -> Option<Duration> {
        Some(Duration::from_secs(90))
    }

    /// Default value for [`Self::pool_max_idle_per_host`]
    #[must_use]
    #[inline]
    fn default_pool_max_idle_per_host() -> usize {
        std::usize::MAX
    }

    /// Build a value for `User-Agent` header
    #[must_use]
    fn user_agent(&self) -> Option<HeaderValue> {
        const UXUM_PRODUCT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
        if let Some(app_name) = &self.app_name {
            let val = if let Some(app_version) = &self.app_version {
                let app_product = [app_name.as_str(), app_version.as_str()].join("/");
                [&app_product, UXUM_PRODUCT].join(" ")
            } else {
                [app_name, UXUM_PRODUCT].join(" ")
            };
            HeaderValue::from_str(&val).ok()
        } else {
            HeaderValue::from_str(UXUM_PRODUCT).ok()
        }
    }

    /// Set short name of an application
    ///
    /// Whitespace is not allowed, as this value is used in User-Agent: HTTP header, among other
    /// things.
    pub fn with_app_name(&mut self, app_name: impl ToString) -> &mut Self {
        // TODO: maybe check for value correctness?
        self.app_name = Some(app_name.to_string());
        self
    }

    /// Set application version
    ///
    /// Preferably in semver format. Whitespace is not allowed, as this value is used in Server:
    /// HTTP header, among other things.
    pub fn with_app_version(&mut self, app_version: impl ToString) -> &mut Self {
        // TODO: maybe check for value correctness?
        self.app_version = Some(app_version.to_string());
        self
    }

    /// Create [`reqwest::ClientBuilder`] from configuration
    pub async fn to_client_builder(&self) -> Result<ClientBuilder, HttpClientError> {
        let mut builder = ClientBuilder::new()
            .use_rustls_tls()
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            .connection_verbose(self.verbose)
            .pool_idle_timeout(self.pool_idle_timeout)
            .pool_max_idle_per_host(self.pool_max_idle_per_host)
            .tls_sni(true)
            .redirect(self.redirect.into())
            .referer(self.referer)
            .tcp_nodelay(self.tcp.nodelay)
            .tcp_keepalive(self.tcp.keepalive)
            .http2_adaptive_window(self.http2.adaptive_window)
            .http2_initial_connection_window_size(
                self.http2.initial_connection_window.map(NonZeroU32::get),
            )
            .http2_initial_stream_window_size(self.http2.initial_stream_window.map(NonZeroU32::get))
            .http2_keep_alive_interval(self.http2.keepalive.interval)
            .http2_keep_alive_while_idle(self.http2.keepalive.while_idle)
            .http2_max_frame_size(self.http2.max_frame_size.map(NonZeroU32::get));
        if let Some(client_cert) = &self.client_cert {
            builder = builder.identity(load_identity(client_cert).await?);
        }
        if let Some(connect_timeout) = self.connect_timeout {
            builder = builder.connect_timeout(connect_timeout);
        }
        if let Some(read_timeout) = self.read_timeout {
            builder = builder.read_timeout(read_timeout);
        }
        if let Some(request_timeout) = self.request_timeout {
            builder = builder.timeout(request_timeout);
        }
        if !self.extra_headers.is_empty() {
            builder = builder.default_headers(
                self.extra_headers
                    .iter()
                    .filter_map(|(k, v)| {
                        Some((
                            HeaderName::from_str(k).ok()?,
                            HeaderValue::from_str(v).ok()?,
                        ))
                    })
                    .collect(),
            );
        }
        if let Some(timeout) = self.http2.keepalive.timeout {
            builder = builder.http2_keep_alive_timeout(timeout);
        }
        if let Some(user_agent) = self.user_agent() {
            builder = builder.user_agent(user_agent);
        }
        Ok(builder)
    }

    /// Convert passed client builder into client with all necessary middlewares attached
    pub fn build_client(
        &self,
        builder: ClientBuilder,
    ) -> Result<ClientWithMiddleware, HttpClientError> {
        // TODO: forward X-Request-Id
        // TODO: forward X-Timeout
        Ok(wrap_client(builder.build()?))
    }

    /// Build and return configured [`reqwest`] HTTP client
    pub async fn to_client(&self) -> Result<ClientWithMiddleware, HttpClientError> {
        self.build_client(self.to_client_builder().await?)
    }
}

/// TCP-level configuration
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct HttpClientTcpConfig {
    /// Set whether sockets have `TCP_NODELAY` enabled
    ///
    /// Default is `true`.
    #[serde(default = "crate::util::default_true")]
    pub nodelay: bool,
    ///
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub keepalive: Option<Duration>,
}

impl Default for HttpClientTcpConfig {
    fn default() -> Self {
        Self {
            nodelay: true,
            keepalive: None,
        }
    }
}

/// HTTP/2 protocol configuration
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct HttpClientHttp2Config {
    /// Sets whether to use an HTTP/2 adaptive flow control
    ///
    /// Enabling this will override the limits set in
    /// [`Self::initial_stream_window`] and [`Self::initial_connection_window`].
    ///
    /// Default is `false`.
    #[serde(default)]
    pub adaptive_window: bool,
    /// Sets the max connection-level flow control for HTTP/2
    ///
    /// Default is currently 65535 but may change internally to optimize for common uses.
    // TODO: bytesize
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_connection_window: Option<NonZeroU32>,
    /// Sets the `SETTINGS_INITIAL_WINDOW_SIZE` option for HTTP/2 stream-level flow control
    ///
    /// Default is currently 65535 but may change internally to optimize for common uses.
    // TODO: bytesize
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_stream_window: Option<NonZeroU32>,
    /// HTTP/2 keep-alive configuration
    #[serde(default)]
    pub keepalive: HttpClientHttp2KeepaliveConfig,
    /// Sets the maximum frame size to use for HTTP/2
    ///
    /// Default is currently 16384 but may change internally to optimize for common uses.
    // TODO: bytesize
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_frame_size: Option<NonZeroU32>,
}

/// HTTP/2 keepalive configuration
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct HttpClientHttp2KeepaliveConfig {
    ///
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub interval: Option<Duration>,
    ///
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub timeout: Option<Duration>,
    ///
    #[serde(default)]
    pub while_idle: bool,
}

///
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum HttpClientRedirectPolicy {
    ///
    None,
    ///
    Limited {
        ///
        #[serde(flatten, default = "HttpClientRedirectPolicy::default_redirect_limit")]
        redirect_limit: usize,
    },
}

impl Default for HttpClientRedirectPolicy {
    fn default() -> Self {
        Self::Limited {
            redirect_limit: Self::default_redirect_limit(),
        }
    }
}

impl From<HttpClientRedirectPolicy> for reqwest::redirect::Policy {
    fn from(value: HttpClientRedirectPolicy) -> Self {
        match value {
            HttpClientRedirectPolicy::None => Self::none(),
            HttpClientRedirectPolicy::Limited { redirect_limit } => Self::limited(redirect_limit),
        }
    }
}

impl HttpClientRedirectPolicy {
    /// Default value for [`Self::Limited::redirect_limit`]
    #[must_use]
    #[inline]
    fn default_redirect_limit() -> usize {
        10
    }
}

///
async fn load_identity(pem_file: &Path) -> Result<Identity, HttpClientError> {
    let mut pem_buf = Vec::new();
    OpenOptions::new()
        .read(true)
        .open(pem_file)
        .await
        .map_err(HttpClientError::identity_load)?
        .read_to_end(&mut pem_buf)
        .await
        .map_err(HttpClientError::identity_load)?;
    Identity::from_pem(&pem_buf).map_err(Into::into)
}
