use zeroize::{Zeroize, ZeroizeOnDrop};

/// Autoentication tokens to verify.
#[derive(Zeroize, ZeroizeOnDrop)]
pub enum AuthToken {
    /// Token is verified externally, always accept.
    ExternallyVerified,
    /// Plaintext password to compare with auth data provider.
    PlainPassword(String),
    // TODO: HashedPassword, SaltedHashedPassword, HmacPassword, CRAM/SCRAM/Digest?
}

impl AuthToken {
    ///
    pub fn compare_plaintext(&self, other: impl AsRef<str>) -> bool {
        let other = other.as_ref();
        match self {
            Self::ExternallyVerified => true,
            Self::PlainPassword(pwd) => pwd == other,
        }
    }
}

impl From<String> for AuthToken {
    fn from(item: String) -> Self {
        Self::PlainPassword(item)
    }
}
