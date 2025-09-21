//! AAA - providers.

use std::sync::Arc;

use crate::auth::{config::AuthConfig, errors::AuthError, token::AuthToken, user::UserId};

/// Authentication provider (back-end) trait.
pub trait AuthProvider: Clone + Send {
    /// User ID type.
    ///
    /// Acquired from auth extractor (front-end) for authentication and authorization.
    /// On successful authentication it is injected into request as an extension.
    type User: Clone + Send + Sync + 'static;
    /// Authentication data type.
    type AuthTokens;

    /// Authenticate the request.
    ///
    /// This checks if user exists, and verifies that any passed auth tokens are valid.
    ///
    /// # Errors
    ///
    /// Returns `Err` if user authentication failed, or on other error condition.
    fn authenticate(&self, user: &Self::User, tokens: &Self::AuthTokens) -> Result<(), AuthError>;

    /// Authorize the request.
    ///
    /// Checks if the user has specific permission.
    ///
    /// # Errors
    ///
    /// Returns `Err` if permission check is unsuccessful, or on other error condition.
    fn authorize(&self, user: &Self::User, permission: &'static str) -> Result<(), AuthError>;
}

/// Authentication provider (back-end) which does nothing.
#[derive(Clone, Debug, Default)]
pub struct NoOpAuthProvider;

impl AuthProvider for NoOpAuthProvider {
    type User = ();
    type AuthTokens = ();

    fn authenticate(
        &self,
        _user: &Self::User,
        _tokens: &Self::AuthTokens,
    ) -> Result<(), AuthError> {
        Ok(())
    }

    fn authorize(&self, _user: &Self::User, _permission: &'static str) -> Result<(), AuthError> {
        Ok(())
    }
}

/// Authentication provider (back-end) that uses users and roles stored in app configuration.
#[derive(Clone, Debug)]
pub struct ConfigAuthProvider {
    /// Authentication database.
    ///
    /// Contains all user and role definitions.
    config: Arc<AuthConfig>,
}

impl AuthProvider for ConfigAuthProvider {
    type User = UserId;
    type AuthTokens = AuthToken;

    fn authenticate(&self, user: &Self::User, tokens: &Self::AuthTokens) -> Result<(), AuthError> {
        match (self.config.user(user), tokens) {
            (_, AuthToken::ExternallyVerified) => Ok(()),
            (Some(user_cfg), AuthToken::PlainPassword(pwd)) => {
                if user_cfg.password.as_ref().is_some_and(|p| p == pwd.as_str()) {
                    Ok(())
                } else {
                    Err(AuthError::AuthFailed)
                }
            }
            (None, _) => Err(AuthError::UserNotFound),
        }
    }

    fn authorize(&self, user: &Self::User, permission: &'static str) -> Result<(), AuthError> {
        // TODO: combine with authentication to avoid double lookup
        match self.config.user(user) {
            Some(user_cfg) => {
                for role in &user_cfg.roles {
                    if let Some(role_cfg) = self.config.roles.get(role) {
                        if role_cfg.super_user || role_cfg.permissions.contains(permission) {
                            return Ok(());
                        }
                    }
                }
                Err(AuthError::NoPermission(permission))
            }
            None => Err(AuthError::UserNotFound),
        }
    }
}

impl From<AuthConfig> for ConfigAuthProvider {
    fn from(value: AuthConfig) -> Self {
        Self {
            config: Arc::new(value),
        }
    }
}
