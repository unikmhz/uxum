//! AAA - authentication token object.

use deboog::Deboog;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Autoentication tokens to verify.
#[derive(Clone, Deboog, Default, Zeroize, ZeroizeOnDrop)]
pub enum AuthToken {
    /// No verifiable tokens were provided.
    #[default]
    Absent,
    /// Token is verified externally, always accept.
    ExternallyVerified,
    /// Plaintext password to compare with auth data provider.
    PlainPassword(#[deboog(mask = "hidden")] String),
    // TODO: HashedPassword, SaltedHashedPassword, HmacPassword, CRAM/SCRAM/Digest?
}

impl From<String> for AuthToken {
    fn from(item: String) -> Self {
        Self::PlainPassword(item)
    }
}
