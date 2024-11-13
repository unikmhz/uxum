//! AAA - user object.

use std::{
    borrow::{Borrow, BorrowMut},
    ops::{Deref, DerefMut},
};

/// User ID.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[repr(transparent)]
pub struct UserId(String);

impl From<String> for UserId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for UserId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl Deref for UserId {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for UserId {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Borrow<str> for UserId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl BorrowMut<str> for UserId {
    fn borrow_mut(&mut self) -> &mut str {
        &mut self.0
    }
}
