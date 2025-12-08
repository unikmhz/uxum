//! SPIFFE authentication and authorization support.

use std::{collections::BTreeSet, env, num::NonZeroUsize, sync::Arc, time::Duration};

use axum_server::tls_rustls::RustlsConfig;
use serde::{Deserialize, Serialize};
use spiffe::{SpiffeIdError, X509ResourceLimits, X509Source, X509SourceError};
use spiffe_rustls::{
    Error as SpiffeRustlsError, TrustDomain, TrustDomainPolicy, authorizer, mtls_server,
};
use thiserror::Error;
use url::Url;

use crate::metrics::SpiffeMetrics;

/// SPIFFE-related error.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SpiffeError {
    /// Invalid SPIFFE ID.
    #[error("Invalid SPIFFE ID: {0}")]
    Id(#[from] SpiffeIdError),
    /// Error setting up SPIFFE X.509 source.
    #[error("Error setting up SPIFFE X.509 source: {0}")]
    Source(#[from] X509SourceError),
    /// Error setting up SPIFFE TLS config.
    #[error("Error setting up SPIFFE TLS config: {0}")]
    Rustls(#[from] SpiffeRustlsError),
    /// Cannot find SPIFFE workload API endpoint.
    #[error("Cannot find SPIFFE workload API endpoint: {0}")]
    NoEndpoint(String),
}

/// SPIFFE configuration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SpiffeConfig {
    /// SPIFFE local resource limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<SpiffeResourceLimits>,
    /// Custom SPIFFE workload API endpoint.
    ///
    /// Typically contains `unix://` URL to a socket.
    ///
    /// Default value is `unix:///tmp/spire-agent/public/api.sock`.
    ///
    /// Explicitly specify no value to force use of `SPIFFE_ENDPOINT_SOCKET`
    /// environment variable.
    #[serde(
        default = "SpiffeConfig::default_workload_api",
        skip_serializing_if = "Option::is_none"
    )]
    pub workload_api: Option<Url>,
    /// Optional timeout for initial sync of SPIFFE state and retrieval of
    /// bundle sets.
    ///
    /// Unbounded by default.
    #[serde(default, with = "humantime_serde")]
    pub initial_sync_timeout: Option<Duration>,
    /// Wait time to finish background tasks for source.
    ///
    /// Default is 30 seconds.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub shutdown_timeout: Option<Duration>,
    /// Reconnect interval settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconnect: Option<SpiffeReconnectBackoff>,
    /// SPIFFE authorizer settings.
    #[serde(default)]
    pub authorize: SpiffeAuthorize,
    /// SPIFFE trust domain policy.
    #[serde(default)]
    pub trust_domain_policy: SpiffeTrustDomainPolicy,
    /// Protocols to consider for negotiation using TLS ALPN.
    #[serde(default = "SpiffeConfig::default_alpn_protocols")]
    pub alpn_protocols: Vec<TlsAlpnProtocol>,
}

impl Default for SpiffeConfig {
    fn default() -> Self {
        Self {
            limits: None,
            workload_api: Self::default_workload_api(),
            initial_sync_timeout: None,
            shutdown_timeout: None,
            reconnect: None,
            authorize: SpiffeAuthorize::default(),
            trust_domain_policy: SpiffeTrustDomainPolicy::default(),
            alpn_protocols: Self::default_alpn_protocols(),
        }
    }
}

impl SpiffeConfig {
    /// Default value for [`Self::workload_api`].
    #[must_use]
    #[inline]
    fn default_workload_api() -> Option<Url> {
        // SAFETY: static value always parses successfully.
        Some(Url::parse("unix:///tmp/spire-agent/public/api.sock").unwrap())
    }

    /// Default value for [`Self::alpn_protocols`].
    #[must_use]
    #[inline]
    fn default_alpn_protocols() -> Vec<TlsAlpnProtocol> {
        vec![TlsAlpnProtocol::Http2, TlsAlpnProtocol::Http11]
    }

    /// Generate SPIFFE X.509 source object from configuration.
    ///
    /// # Errors
    ///
    /// Returns `Err` if provided SPIFFE configuration is invalid.
    pub async fn build_source(
        &self,
        metrics: Option<SpiffeMetrics>,
    ) -> Result<X509Source, SpiffeError> {
        let mut builder = X509Source::builder();
        if let Some(ref limits) = self.limits {
            builder = builder.resource_limits(limits.clone().into());
        }
        if let Some(ref endpoint) = self.workload_api {
            builder = builder.endpoint(endpoint);
        } else if let Err(err) = env::var("SPIFFE_ENDPOINT_SOCKET") {
            return Err(SpiffeError::NoEndpoint(err.to_string()));
        }
        if let Some(timeout) = self.initial_sync_timeout {
            builder = builder.initial_sync_timeout(timeout);
        }
        if let Some(timeout) = self.shutdown_timeout {
            builder = builder.shutdown_timeout(Some(timeout));
        }
        if let Some(SpiffeReconnectBackoff {
            min_backoff,
            max_backoff,
        }) = self.reconnect
        {
            builder = builder.reconnect_backoff(min_backoff, max_backoff);
        }
        if let Some(metrics) = metrics {
            builder = builder.metrics(metrics.into_arc_inner());
        }
        builder.build().await.map_err(Into::into)
    }

    /// Generate configuration object for RusTLS.
    ///
    /// # Errors
    ///
    /// Returns `Err` if provided SPIFFE configuration is invalid.
    pub async fn rustls_config(
        &self,
        metrics: Option<SpiffeMetrics>,
    ) -> Result<RustlsConfig, SpiffeError> {
        let source = self.build_source(metrics).await?;
        let mut builder = mtls_server(source);
        builder = match &self.authorize {
            SpiffeAuthorize::Any => builder.authorize(authorizer::any()),
            SpiffeAuthorize::Exact { spiffe_ids } => {
                builder.authorize(authorizer::exact(spiffe_ids.iter().map(|s| s.as_str()))?)
            }
            SpiffeAuthorize::TrustDomains { trust_domains } => builder.authorize(
                authorizer::trust_domains(trust_domains.iter().map(|s| s.as_str()))?,
            ),
        };
        builder = builder.trust_domain_policy(self.trust_domain_policy.clone().try_into()?);
        let config = builder.with_alpn_protocols(&self.alpn_protocols).build()?;
        Ok(RustlsConfig::from_config(Arc::new(config)))
    }
}

/// SPIFFE resource limits for SVID and bundle storage to prevent resource exhaustion.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SpiffeResourceLimits {
    /// Maximum number of SVIDs allowed in a context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_svids: Option<NonZeroUsize>,
    /// Maximum number of bundles allowed in a bundle set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bundles: Option<NonZeroUsize>,
    /// Maximum bundle DER size in bytes (per bundle).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bundle_der_bytes: Option<NonZeroUsize>,
}

impl From<SpiffeResourceLimits> for X509ResourceLimits {
    fn from(value: SpiffeResourceLimits) -> Self {
        Self::new(
            value.max_svids.map(NonZeroUsize::get),
            value.max_bundles.map(NonZeroUsize::get),
            value.max_bundle_der_bytes.map(NonZeroUsize::get),
        )
    }
}

/// Backoff interval configuration between subsequent workload API connection attempts.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SpiffeReconnectBackoff {
    /// Minimum interval between reconnect attempts.
    #[serde(with = "humantime_serde")]
    min_backoff: Duration,
    /// Maximum interval between reconnect attempts.
    #[serde(with = "humantime_serde")]
    max_backoff: Duration,
}

/// SPIFFE authorizer configuration.
///
/// Limits Spiffe IDs that can connect to server.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SpiffeAuthorize {
    /// Allow any SPIFFE ID.
    #[default]
    Any,
    /// Allow only exact SPIFFE IDs listed.
    Exact {
        /// Allowed SPIFFE IDs.
        spiffe_ids: Vec<String>,
    },
    /// Allow SPIFFE IDs from listed trust domains.
    TrustDomains {
        /// Allowed trust domains.
        trust_domains: Vec<String>,
    },
}

/// Policy for selecting which trust domains to trust during certificate verification.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SpiffeTrustDomainPolicy {
    /// Default: use all trust domain bundles provided by the Workload API.
    #[default]
    #[serde(alias = "any")]
    AnyInBundleSet,
    /// Only trust the specified trust domain.
    #[serde(alias = "single", alias = "local")]
    LocalOnly {
        /// Allowed trust domain.
        domain: String,
    },
    /// Restrict to these trust domains only.
    #[serde(alias = "list")]
    AllowList {
        /// Allowed trust domains.
        domains: Vec<String>,
    },
}

impl TryFrom<SpiffeTrustDomainPolicy> for TrustDomainPolicy {
    type Error = SpiffeIdError;

    fn try_from(value: SpiffeTrustDomainPolicy) -> Result<Self, Self::Error> {
        match value {
            SpiffeTrustDomainPolicy::AnyInBundleSet => Ok(Self::AnyInBundleSet),
            SpiffeTrustDomainPolicy::LocalOnly { domain } => {
                Ok(Self::LocalOnly(domain.try_into()?))
            }
            SpiffeTrustDomainPolicy::AllowList { domains } => {
                let domains: Result<BTreeSet<TrustDomain>, Self::Error> =
                    domains.into_iter().map(TryFrom::try_from).collect();
                Ok(Self::AllowList(domains?))
            }
        }
    }
}

/// Protocol to consider for negotiation using TLS ALPN.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum TlsAlpnProtocol {
    /// HTTP/1.1.
    #[default]
    #[serde(alias = "http", alias = "http1", alias = "http/1", alias = "http/1.1")]
    Http11,
    /// HTTP/2. Required for gRPC.
    #[serde(alias = "http/2")]
    Http2,
}

impl AsRef<[u8]> for TlsAlpnProtocol {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Http11 => b"http/1.1",
            Self::Http2 => b"h2",
        }
    }
}
