use std::{borrow::Borrow, fmt, ops::Deref};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct HandlerName(&'static str);

impl HandlerName {
    pub fn new(name: &'static str) -> Self {
        Self(name)
    }

    pub fn as_str(&self) -> &'static str {
        self.0
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

impl fmt::Display for HandlerName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
