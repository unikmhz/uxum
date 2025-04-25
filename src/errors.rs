//! Error utility functions and types.

use std::{fmt, io};

/// Wrapper for [`std::io::Error`].
#[derive(Debug)]
#[repr(transparent)]
pub struct IoError(io::Error);

impl From<io::Error> for IoError {
    fn from(value: io::Error) -> Self {
        Self(value)
    }
}

impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}, kind={:?}", self.0, self.0.kind())?;
        if let Some(raw) = self.0.raw_os_error() {
            write!(f, ", raw={raw}")?;
        }
        Ok(())
    }
}

/// Tag URI for uxum 404 bodies.
pub(crate) const TAG_UXUM_NOT_FOUND: &str = "tag:uxum.github.io,2024:not_found";
/// Tag URI for uxum error bodies.
pub(crate) const TAG_UXUM_ERROR: &str = "tag:uxum.github.io,2024:error";
/// Tag URI for uxum panic bodies.
pub(crate) const TAG_UXUM_PANIC: &str = "tag:uxum.github.io,2024:panic";
/// Tag URI for uxum rate-limiting result.
pub(crate) const TAG_UXUM_RATE_LIMIT: &str = "tag:uxum.github.io,2024:rate-limit";
/// Tag URI for uxum auth error bodies.
pub(crate) const TAG_UXUM_AUTH: &str = "tag:uxum.github.io,2024:auth";
/// Tag URI for uxum timeout error bodies.
pub(crate) const TAG_UXUM_TIMEOUT: &str = "tag:uxum.github.io,2024:timeout";
/// Tag URI for uxum metrics error bodies.
pub(crate) const TAG_UXUM_METRICS: &str = "tag:uxum.github.io,2024:metrics";
