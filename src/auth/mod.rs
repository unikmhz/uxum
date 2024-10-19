//! Authentication and authorization system.

mod config;
mod errors;
mod extractor;
mod layer;
mod provider;
mod user;

pub use self::{
    config::{AuthConfig, RoleConfig, UserConfig, UserPassword},
    errors::AuthError,
    extractor::{AuthExtractor, BasicAuthExtractor, HeaderAuthExtractor, NoOpAuthExtractor},
    layer::AuthLayer,
    provider::{AuthProvider, ConfigAuthProvider, NoOpAuthProvider},
    user::UserId,
};
