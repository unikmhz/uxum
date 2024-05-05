use std::{fmt, io};

/// Wrapper for [`std::io::Error`]
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
