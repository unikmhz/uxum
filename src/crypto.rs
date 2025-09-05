//! A set of utility functions for working with crypto providers.

/// Ensure that the default crypto provider is installed.
/// It is ok for `install_default()` to fail if the crypto provider is already installed.
pub fn ensure_default_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}
