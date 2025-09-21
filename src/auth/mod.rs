//! Authentication and authorization system.

mod config;
mod errors;
mod extractor;
mod layer;
mod provider;
mod token;
mod user;

pub use self::{
    config::{AuthConfig, RoleConfig, UserConfig, UserPassword},
    errors::AuthError,
    extractor::{AuthExtractor, BasicAuthExtractor, HeaderAuthExtractor, NoOpAuthExtractor},
    layer::AuthLayer,
    provider::{AuthProvider, ConfigAuthProvider, NoOpAuthProvider},
    token::AuthToken,
    user::UserId,
};
