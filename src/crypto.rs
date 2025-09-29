//! A set of utility functions for working with crypto providers and configuration.

use std::path::PathBuf;

use opentelemetry_otlp::tonic_types::transport::{Certificate, ClientTlsConfig, Identity};
use serde::{Deserialize, Serialize};

use crate::util::fs::read_file;

/// Ensure that the default crypto provider is installed.
/// It is ok for `install_default()` to fail if the crypto provider is already installed.
pub fn ensure_default_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

/// TLS configuration for OpenTelemetry exporters.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct TonicTlsConfig {
    /// Require server certificate to be issued against specified domain name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// List of CA certificates to load for the purpose of server certificate verification.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ca_certs: Vec<PathBuf>,
    /// Client
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<TlsIdentity>,
    /// Assume the server supports HTTP/2 even when it doesn't provide protocol negotiation via
    /// ALPN.
    #[serde(default)]
    pub assume_http2: bool,
    /// Enable platform-native TLS root store.
    #[serde(default = "crate::util::default_true")]
    with_native_roots: bool,
    /// Use key log as specified by the `SSLKEYLOGFILE` environment variable.
    #[serde(default)]
    pub use_key_log: bool,
}

impl Default for TonicTlsConfig {
    fn default() -> Self {
        Self {
            domain: None,
            ca_certs: Vec::new(),
            identity: None,
            assume_http2: false,
            with_native_roots: true,
            use_key_log: false,
        }
    }
}

impl TonicTlsConfig {
    /// Create TLS client configuration for use with [`tonic`].
    ///
    /// # Errors
    ///
    /// This call will error out if some certificate or key files could not be loaded from
    /// filesystem.
    pub async fn to_tonic_config(&self) -> Result<ClientTlsConfig, std::io::Error> {
        let mut cfg = ClientTlsConfig::new().assume_http2(self.assume_http2);
        if let Some(domain) = &self.domain {
            cfg = cfg.domain_name(domain);
        }
        for path in &self.ca_certs {
            let buf = read_file(path).await?;
            cfg = cfg.ca_certificate(Certificate::from_pem(buf));
        }
        if let Some(identity) = &self.identity {
            cfg = cfg.identity(identity.to_tonic_identity().await?);
        }
        if self.with_native_roots {
            cfg = cfg.with_native_roots();
        }
        if self.use_key_log {
            cfg = cfg.use_key_log();
        }
        Ok(cfg)
    }
}

/// Client certificate and private key to use.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct TlsIdentity {
    /// Path to client certificate.
    cert: PathBuf,
    /// Path to client private key.
    key: PathBuf,
}

impl TlsIdentity {
    /// Create identity object for use in [`tonic`] configuration.
    ///
    /// # Errors
    ///
    /// Returns `Err` if unable to load certificate or key files.
    pub async fn to_tonic_identity(&self) -> Result<Identity, std::io::Error> {
        let cert = read_file(&self.cert).await?;
        let key = read_file(&self.key).await?;
        Ok(Identity::from_pem(cert, key))
    }
}
