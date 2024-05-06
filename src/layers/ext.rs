//! Miscellaneous types used as request extensions

use std::{
    borrow::{Borrow, BorrowMut},
    fmt,
    hash::Hash,
    ops::{Deref, DerefMut},
    time::{Duration, Instant},
};

/// Static handler name
///
/// This gets attached as an extension to requests and responses for use mainly in middleware
/// layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct HandlerName(&'static str);

impl HandlerName {
    /// Construct new [`HandlerName`] from static string slice
    #[must_use]
    pub fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// Get static string slice stored inside
    #[must_use]
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

    fn deref(&self) -> &Self::Target {
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

/// Cutoff time after which the request must be timed out
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Deadline(Instant);

impl Deadline {
    /// Construct new [`Deadline`] with zero time left
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if deadline has passed
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.0 >= Instant::now()
    }

    /// Get remaining time
    ///
    /// Returns [`None`] if deadline has passed.
    #[must_use]
    pub fn time_left(&self) -> Option<Duration> {
        Instant::now().checked_duration_since(self.0)
    }
}

impl Default for Deadline {
    fn default() -> Self {
        Self(Instant::now())
    }
}

impl From<Duration> for Deadline {
    fn from(value: Duration) -> Self {
        Self(Instant::now() + value)
    }
}

impl AsRef<Instant> for Deadline {
    fn as_ref(&self) -> &Instant {
        &self.0
    }
}

impl Deref for Deadline {
    type Target = Instant;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Deadline {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Borrow<Instant> for Deadline {
    fn borrow(&self) -> &Instant {
        &self.0
    }
}

impl BorrowMut<Instant> for Deadline {
    fn borrow_mut(&mut self) -> &mut Instant {
        &mut self.0
    }
}

impl fmt::Display for Deadline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}s left",
            self.time_left().unwrap_or_default().as_secs_f64()
        )
    }
}
