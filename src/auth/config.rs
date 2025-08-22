//! AAA - configuration.

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::{Deref, DerefMut},
};

use argon2::{Argon2, PasswordVerifier};
use password_hash::PasswordHashString;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

/// User configuration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct UserConfig {
    /// User password value.
    #[serde(flatten)]
    pub password: UserPassword,
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

impl PartialEq<&str> for UserPassword {
    fn eq(&self, other: &&str) -> bool {
        match self {
            Self::Plaintext(pwd) => pwd.as_bytes().ct_eq(other.as_bytes()).into(),
            // FIXME: generalize hash verification.
            Self::Hashed(pwd) => Argon2::default()
                .verify_password(other.as_bytes(), &pwd.password_hash())
                .is_ok(),
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
}

impl AuthConfig {
    /// Find and return user by name.
    pub fn user(&self, name: &str) -> Option<&UserConfig> {
        self.users.get(name)
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
