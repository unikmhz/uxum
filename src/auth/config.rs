//! AAA - configuration.

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::{Deref, DerefMut},
    sync::OnceLock,
};
#[cfg(feature = "jwt")]
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(feature = "jwt")]
use jsonwebtoken as jwt;
use password_hash::{PasswordHashString, PasswordVerifier};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

#[cfg(feature = "jwt")]
use crate::auth::extractor::JwtAuthExtractor;
use crate::auth::{
    errors::AuthSetupError,
    extractor::{AuthExtractor, BasicAuthExtractor, HeaderAuthExtractor, NoOpAuthExtractor},
    provider::{AuthProvider, ConfigAuthProvider, NoOpAuthProvider},
};

static VERIFIERS: OnceLock<Vec<Box<dyn PasswordVerifier + Send + Sync>>> = OnceLock::new();

fn get_verifiers() -> &'static Vec<Box<dyn PasswordVerifier + Send + Sync>> {
    VERIFIERS.get_or_init(|| {
        vec![
            #[cfg(feature = "hash_argon2")]
            Box::new(argon2::Argon2::default()),
            #[cfg(feature = "hash_scrypt")]
            Box::new(scrypt::Scrypt),
            #[cfg(feature = "hash_pbkdf2")]
            Box::new(pbkdf2::Pbkdf2),
        ]
    })
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
                pwd.password_hash()
                    .verify_password(verifiers.as_slice(), other.as_bytes())
                    .is_ok()
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        realm: Option<String>,
    },
    /// Extract credentials using (configurable) HTTP headers.
    Header {
        /// Custom user HTTP header to use, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user_header: Option<String>,
        /// Custom token HTTP header to use, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token_header: Option<String>,
    },
    #[cfg(feature = "jwt")]
    /// Extract credentials using HTTP Bearer containing a JWT.
    Jwt {
        /// Custom realm to use for HTTP Bearer auth, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        realm: Option<String>,
        /// Signature algorithm to use when verifying JWS.
        #[serde(default, alias = "algorithm")]
        algo: JwtAlgorithm,
        /// Key to use for JWS verification.
        key: JwtKey,
        /// JWT validation parameters.
        #[serde(default)]
        validate: JwtValidation,
    },
    /// Use several extractors, trying each one in sequence until there is success.
    Stacked {
        /// Contents of the extractor stack.
        extractors: Vec<ExtractorConfig>,
    },
}

impl ExtractorConfig {
    /// Construct an extractor based on configuration.
    ///
    /// # Errors
    ///
    /// Returns `Err` if some key data failed to load or decode.
    pub fn make_extractor(&self) -> Result<Box<dyn AuthExtractor>, AuthSetupError> {
        // TODO: make this async
        match self {
            Self::NoOp => Ok(Box::new(NoOpAuthExtractor)),
            Self::Basic { realm } => Ok(Box::new(BasicAuthExtractor::new(realm.as_deref()))),
            Self::Header {
                user_header,
                token_header,
            } => Ok(Box::new(HeaderAuthExtractor::new(
                user_header.as_deref(),
                token_header.as_deref(),
            ))),
            #[cfg(feature = "jwt")]
            Self::Jwt {
                realm,
                algo,
                key,
                validate,
            } => Ok(Box::new(JwtAuthExtractor::new(
                realm.as_deref(),
                key.to_key()?,
                validate.to_validation(*algo),
            ))),
            Self::Stacked { .. } => todo!("Stacked auth unimplemented"),
        }
    }
}

#[cfg(feature = "jwt")]
/// Used JWT signature algorithm.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum JwtAlgorithm {
    /// HMAC using SHA-256.
    #[default]
    Hs256,
    /// HMAC using SHA-384.
    Hs384,
    /// HMAC using SHA-512.
    Hs512,
    /// ECDSA using SHA-256.
    Es256,
    /// ECDSA using SHA-384.
    Es384,
    /// RSASSA-PKCS1-v1_5 using SHA-256.
    Rs256,
    /// RSASSA-PKCS1-v1_5 using SHA-384.
    Rs384,
    /// RSASSA-PKCS1-v1_5 using SHA-512.
    Rs512,
    /// RSASSA-PSS using SHA-256.
    Ps256,
    /// RSASSA-PSS using SHA-384.
    Ps384,
    /// RSASSA-PSS using SHA-512.
    Ps512,
    /// Edwards-curve Digital Signature Algorithm.
    #[serde(rename = "EdDSA")]
    EdDSA,
}

#[cfg(feature = "jwt")]
impl From<JwtAlgorithm> for jwt::Algorithm {
    fn from(value: JwtAlgorithm) -> Self {
        match value {
            JwtAlgorithm::Hs256 => Self::HS256,
            JwtAlgorithm::Hs384 => Self::HS384,
            JwtAlgorithm::Hs512 => Self::HS512,
            JwtAlgorithm::Es256 => Self::ES256,
            JwtAlgorithm::Es384 => Self::ES384,
            JwtAlgorithm::Rs256 => Self::RS256,
            JwtAlgorithm::Rs384 => Self::RS384,
            JwtAlgorithm::Rs512 => Self::RS512,
            JwtAlgorithm::Ps256 => Self::PS256,
            JwtAlgorithm::Ps384 => Self::PS384,
            JwtAlgorithm::Ps512 => Self::PS512,
            JwtAlgorithm::EdDSA => Self::EdDSA,
        }
    }
}

#[cfg(feature = "jwt")]
/// Customize JWT validation process.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JwtValidation {
    /// Claims that need to be present.
    ///
    /// Note: currently, only "exp", "nbf", "aud", "iss" and "sub" are supported.
    #[serde(default = "JwtValidation::default_required_claims")]
    required_claims: Vec<String>,
    /// Margins for "exp"/"nbf" validation to account for clock skew.
    #[serde(default = "JwtValidation::default_leeway", with = "humantime_serde")]
    leeway: Duration,
    /// Reject tokens expiring in less than this time.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    reject_expiring_in: Option<Duration>,
    /// Validate "exp" claim.
    #[serde(default = "crate::util::default_true")]
    validate_exp: bool,
    /// Validate "nbf" claim.
    #[serde(default)]
    validate_nbf: bool,
    /// Validate "aud" claim.
    #[serde(default = "crate::util::default_true")]
    validate_aud: bool,
    /// Require "aud" to be one of these values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    audience: Vec<String>,
    /// Require "iss" to be one of these values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    issuer: Vec<String>,
}

#[cfg(feature = "jwt")]
impl Default for JwtValidation {
    fn default() -> Self {
        Self {
            required_claims: Self::default_required_claims(),
            leeway: Self::default_leeway(),
            reject_expiring_in: None,
            validate_exp: true,
            validate_nbf: false,
            validate_aud: true,
            audience: Vec::default(),
            issuer: Vec::default(),
        }
    }
}

#[cfg(feature = "jwt")]
impl JwtValidation {
    /// Default value for [`Self::required_claims`].
    #[must_use]
    #[inline]
    fn default_required_claims() -> Vec<String> {
        vec!["exp".into()]
    }

    /// Default value for [`Self::leeway`].
    #[must_use]
    #[inline]
    fn default_leeway() -> Duration {
        Duration::from_secs(60)
    }

    /// Convert these settings into a validation structure.
    pub fn to_validation(&self, algo: JwtAlgorithm) -> jwt::Validation {
        let mut valid = jwt::Validation::new(algo.into());
        valid.set_required_spec_claims(&self.required_claims);
        valid.leeway = self.leeway.as_secs();
        valid.reject_tokens_expiring_in_less_than =
            self.reject_expiring_in.map(|d| d.as_secs()).unwrap_or(0);
        valid.validate_exp = self.validate_exp;
        valid.validate_nbf = self.validate_nbf;
        valid.validate_aud = self.validate_aud;
        if !self.audience.is_empty() {
            valid.set_audience(&self.audience);
        }
        if !self.issuer.is_empty() {
            valid.set_issuer(&self.issuer);
        }
        valid
    }
}

#[cfg(feature = "jwt")]
/// JWT key for signature verification.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JwtKey {
    /// Base64-encoded secret for use with HMAC signatures.
    HmacSecret(String),
    /// RSA public key from PEM-encoded file.
    RsaPem(PathBuf),
    /// RSA public key from DER-encoded file.
    RsaDer(PathBuf),
    /// Base64-encoded RSA public key components.
    RsaComponents { modulus: String, exponent: String },
    /// ECDSA public key from PEM-encoded file.
    EcdsaPem(PathBuf),
    /// ECDSA public key from DER-encoded file.
    EcdsaDer(PathBuf),
    /// Base64-encoded ECDSA public key components.
    EcdsaComponents { x: String, y: String },
    /// EdDSA public key from PEM-encoded file.
    EddsaPem(PathBuf),
    /// EdDSA public key from DER-encoded file.
    EddsaDer(PathBuf),
    /// Base64-encoded EdDSA public key components.
    EddsaComponents { x: String },
}

#[cfg(feature = "jwt")]
impl JwtKey {
    fn path_string(path: &Path) -> String {
        path.as_os_str().to_string_lossy().into_owned()
    }

    fn io_error(path: &Path, err: std::io::Error) -> AuthSetupError {
        AuthSetupError::KeyIo(Self::path_string(path), err.into())
    }

    fn read_path(path: &Path) -> Result<Vec<u8>, AuthSetupError> {
        // TODO: make this async
        let mut file = fs::OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|err| Self::io_error(path, err))?;
        let meta = file.metadata().map_err(|err| Self::io_error(path, err))?;
        let mut buf = Vec::with_capacity(meta.len() as usize);
        let _ = file
            .read_to_end(&mut buf)
            .map_err(|err| Self::io_error(path, err))?;
        Ok(buf)
    }

    /// Load a key for JWS verification.
    pub fn to_key(&self) -> Result<jwt::DecodingKey, AuthSetupError> {
        // TODO: make this async
        match self {
            Self::HmacSecret(s) => jwt::DecodingKey::from_base64_secret(s).map_err(Into::into),
            Self::RsaPem(path) => {
                let buf = Self::read_path(path)?;
                jwt::DecodingKey::from_rsa_pem(&buf).map_err(Into::into)
            }
            Self::RsaDer(path) => {
                let buf = Self::read_path(path)?;
                Ok(jwt::DecodingKey::from_rsa_der(&buf))
            }
            Self::RsaComponents { modulus, exponent } => {
                jwt::DecodingKey::from_rsa_components(modulus, exponent).map_err(Into::into)
            }
            Self::EcdsaPem(path) => {
                let buf = Self::read_path(path)?;
                jwt::DecodingKey::from_ec_pem(&buf).map_err(Into::into)
            }
            Self::EcdsaDer(path) => {
                let buf = Self::read_path(path)?;
                Ok(jwt::DecodingKey::from_ec_der(&buf))
            }
            Self::EcdsaComponents { x, y } => {
                jwt::DecodingKey::from_ec_components(x, y).map_err(Into::into)
            }
            Self::EddsaPem(path) => {
                let buf = Self::read_path(path)?;
                jwt::DecodingKey::from_ed_pem(&buf).map_err(Into::into)
            }
            Self::EddsaDer(path) => {
                let buf = Self::read_path(path)?;
                Ok(jwt::DecodingKey::from_ed_der(&buf))
            }
            Self::EddsaComponents { x } => {
                jwt::DecodingKey::from_ed_components(x).map_err(Into::into)
            }
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
    ///
    /// # Errors
    ///
    /// Returns `Err` if some key data failed to load or decode.
    pub fn make_provider(&self) -> Result<Box<dyn AuthProvider>, AuthSetupError> {
        // TODO: make this async
        match self.provider {
            ProviderConfig::NoOp => Ok(Box::new(NoOpAuthProvider)),
            ProviderConfig::Config => Ok(Box::new(ConfigAuthProvider::from(self.clone()))),
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
