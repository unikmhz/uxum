use std::sync::Arc;

use crate::auth::{config::AuthConfig, errors::AuthError, user::UserId};

pub trait AuthProvider: Clone + Send {
    ///
    type User: Clone + Send + Sync + 'static;
    ///
    type AuthTokens;

    ///
    fn authenticate(&self, user: &Self::User, tokens: &Self::AuthTokens) -> Result<(), AuthError>;
}

///
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
}

///
#[derive(Clone, Debug)]
pub struct ConfigAuthProvider {
    ///
    config: Arc<AuthConfig>,
}

impl AuthProvider for ConfigAuthProvider {
    type User = UserId;
    type AuthTokens = String;

    fn authenticate(&self, user: &Self::User, tokens: &Self::AuthTokens) -> Result<(), AuthError> {
        match self.config.user(user) {
            Some(user_cfg) => {
                if user_cfg.password == tokens.as_str() {
                    Ok(())
                } else {
                    Err(AuthError::AuthFailed)
                }
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
