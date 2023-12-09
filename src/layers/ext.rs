use std::{borrow::Borrow, ops::Deref};

#[derive(Clone, Debug, PartialEq)]
pub struct HandlerName(&'static str);

impl HandlerName {
    pub fn new(name: &'static str) -> Self {
        Self(name)
    }
}

impl AsRef<str> for HandlerName {
    fn as_ref(&self) -> &str {
        self.0
    }
}

impl Deref for HandlerName {
    type Target = str;

    fn deref(&self) -> &<Self as Deref>::Target {
        self.0
    }
}

impl Borrow<str> for HandlerName {
    fn borrow(&self) -> &str {
        self.0
    }
}
