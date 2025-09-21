//! AAA - configuration.

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::{Deref, DerefMut},
    sync::OnceLock,
};

use password_hash::{PasswordHashString, PasswordVerifier};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

use crate::auth::{
    extractor::{AuthExtractor, BasicAuthExtractor, HeaderAuthExtractor, NoOpAuthExtractor},
    provider::{AuthProvider, ConfigAuthProvider, NoOpAuthProvider},
};

static VERIFIERS: OnceLock<Vec<Box<dyn PasswordVerifier + Send + Sync>>> = OnceLock::new();

fn get_verifiers() -> &'static Vec<Box<dyn PasswordVerifier + Send + Sync>> {
    VERIFIERS.get_or_init(|| vec![
        #[cfg(feature = "hash_argon2")]
        Box::new(argon2::Argon2::default()),
        #[cfg(feature = "hash_scrypt")]
        Box::new(scrypt::Scrypt),
        #[cfg(feature = "hash_pbkdf2")]
        Box::new(pbkdf2::Pbkdf2),
    ])
}

/// User configuration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct UserConfig {
    /// User password value.
    #[serde(default, flatten, skip_serializing_if = "Option::is_none")]
    pub password: Option<UserPassword>,
    /// Roles that are granted to this user.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub roles: BTreeSet<String>,
}

/// Various ways of storing client password.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum UserPassword {
    /// Cleartext password value.
    #[serde(rename = "password")]
    Plaintext(String),
    /// Securely hashed password value.
    ///
    /// This accepts strings in PHC format, as defined in [the specification][1].
    ///
    /// Current OWASP recommendation: Argon2id with a minimum configuration of 19 MiB of memory,
    /// an iteration count of 2, and 1 degree of parallelism.
    ///
    /// See [this page][2] for more info.
    ///
    /// [1]: <https://github.com/P-H-C/phc-string-format/blob/master/phc-sf-spec.md> "PHC string format"
    /// [2]: <https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html>
    #[serde(rename = "password_hash", alias = "hash")]
    Hashed(HashedPassword),
}

impl PartialEq<str> for UserPassword {
    fn eq(&self, other: &str) -> bool {
        match self {
            Self::Plaintext(pwd) => other.as_bytes().ct_eq(pwd.as_bytes()).into(),
            // FIXME: pre-parse hashes from configuration.
            Self::Hashed(pwd) => {
                let verifiers = get_verifiers()
                    .iter()
                    .map(|v| v.as_ref() as &dyn PasswordVerifier)
                    .collect::<Vec<_>>();
                pwd.password_hash().verify_password(verifiers.as_slice(), other.as_bytes()).is_ok()
            }
        }
    }
}

/// Role configuration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct RoleConfig {
    /// Permissions that are granted to the role.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub permissions: BTreeSet<String>,
    /// Does this role allow skipping permission checks altogether.
    #[serde(default, skip_serializing_if = "<&bool as std::ops::Not>::not")]
    pub super_user: bool,
}

/// Auth provider (back-end) configuration.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderConfig {
    /// No provider.
    #[default]
    NoOp,
    /// Get user and role information from [`AuthConfig`] struct.
    Config,
}

/// Auth extractor (front-end) configuration.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtractorConfig {
    /// No extractor.
    #[default]
    NoOp,
    /// Extract credentials using HTTP Basic.
    Basic {
        /// Custom realm to use for HTTP Basic auth, if any.
        realm: Option<String>,
    },
    /// Extract credentials using (configurable) HTTP headers.
    Header {
        /// Custom user HTTP header to use, if any.
        user_header: Option<String>,
        /// Custom token HTTP header to use, if any.
        token_header: Option<String>,
    },
    /// Extract credentials using HTTP Bearer containing a JWT.
    Jwt,
    /// Use several extractors, trying each one in sequence until there is success.
    Stacked {
        /// Contents of the extractor stack.
        extractors: Vec<ExtractorConfig>,
    },
}

impl ExtractorConfig {
    /// Construct an extractor based on configuration.
    pub fn make_extractor(&self) -> Box<dyn AuthExtractor> {
        match self {
            Self::NoOp => Box::new(NoOpAuthExtractor),
            Self::Basic { realm } => Box::new(BasicAuthExtractor::new(realm.as_deref())),
            Self::Header { user_header, token_header } => {
                Box::new(HeaderAuthExtractor::new(user_header.as_deref(), token_header.as_deref()))
            }
            Self::Jwt => todo!("JWT auth unimplemented"),
            Self::Stacked { .. } => todo!("Stacked auth unimplemented"),
        }
    }
}

/// Authentication provider configuration.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct AuthConfig {
    /// User dictionary.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub users: BTreeMap<String, UserConfig>,
    /// Role dictionary.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub roles: BTreeMap<String, RoleConfig>,
    /// Configuration for auth data provider to use.
    #[serde(default)]
    pub provider: ProviderConfig,
    /// Configuration for credentials extractor in use.
    #[serde(default)]
    pub extractor: ExtractorConfig,
}

impl AuthConfig {
    /// Find and return user by name.
    pub fn user(&self, name: Option<&str>) -> Option<&UserConfig> {
        name.and_then(|n| self.users.get(n))
    }

    /// Construct a provider based on configuration.
    pub fn make_provider(&self) -> Box<dyn AuthProvider> {
        match self.provider {
            ProviderConfig::NoOp => Box::new(NoOpAuthProvider),
            ProviderConfig::Config => Box::new(ConfigAuthProvider::from(self.clone())),
        }
    }
}

/// Newtype for hashed passwords.
#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct HashedPassword(PasswordHashString);

impl From<PasswordHashString> for HashedPassword {
    fn from(item: PasswordHashString) -> Self {
        Self(item)
    }
}

impl Deref for HashedPassword {
    type Target = PasswordHashString;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for HashedPassword {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

mod serde_impls {
    use std::fmt;

    use serde::{de, Deserializer, Serializer};

    use super::*;

    impl Serialize for HashedPassword {
        fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            ser.serialize_str(self.as_str())
        }
    }

    impl<'de> Deserialize<'de> for HashedPassword {
        fn deserialize<D>(deser: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deser.deserialize_str(HashedPasswordVisitor)
        }
    }

    #[doc(hidden)]
    struct HashedPasswordVisitor;

    impl de::Visitor<'_> for HashedPasswordVisitor {
        type Value = HashedPassword;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a hashed password string using PHC format")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            PasswordHashString::new(v)
                .map(Into::into)
                .map_err(|err| E::custom(format!("unable to parse PHC format: {err}")))
        }
    }
}
