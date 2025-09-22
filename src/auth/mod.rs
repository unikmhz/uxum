//! Authentication and authorization system.

mod config;
mod errors;
mod extractor;
mod layer;
mod provider;
mod token;
mod user;

#[cfg(feature = "jwt")]
pub use self::extractor::JwtAuthExtractor;
pub use self::{
    config::{AuthConfig, ExtractorConfig, ProviderConfig, RoleConfig, UserConfig, UserPassword},
    errors::{AuthError, AuthSetupError},
    extractor::{AuthExtractor, BasicAuthExtractor, HeaderAuthExtractor, NoOpAuthExtractor},
    layer::AuthLayer,
    provider::{AuthProvider, ConfigAuthProvider, NoOpAuthProvider},
    token::AuthToken,
    user::UserId,
};
