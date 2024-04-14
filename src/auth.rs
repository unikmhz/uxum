use std::{
    collections::{HashMap, HashSet},
    ops::{Deref, DerefMut},
};

use password_hash::PasswordHashString;
use serde::{Deserialize, Serialize};

///
trait AuthUser {}

///
trait AuthProvider {}

///
trait AuthMethod {}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct UserConfig {
    ///
    #[serde(flatten)]
    pub password: UserPassword,
    ///
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub roles: HashSet<String>,
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum UserPassword {
    ///
    #[serde(rename = "password")]
    Plaintext(String),
    /// Securely hashed password value
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

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct RoleConfig {
    ///
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub permissions: HashSet<String>,
}

///
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct AuthConfig {
    ///
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub users: HashMap<String, UserConfig>,
    ///
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub roles: HashMap<String, RoleConfig>,
}

impl AuthConfig {
    pub fn user(&self, name: &str) -> Option<&UserConfig> {
        self.users.get(name)
    }
}

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

    impl<'de> de::Visitor<'de> for HashedPasswordVisitor {
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
                .map_err(|err| E::custom(format!("unable to parse PHC format: {}", err)))
        }
    }
}
