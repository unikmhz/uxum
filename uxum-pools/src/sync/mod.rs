//! Support for synchronous pools.

mod impls;
mod pool;

use std::time::Duration;

pub use crate::{error::Error, metrics::PoolState, sync::pool::InstrumentedPool};

/// Pool objects implementing this trait can be instrumented.
pub trait InstrumentablePool<'p> {
    /// Resource type contained in the pool.
    type Resource: 'p;
    /// Error type use by the pool.
    type Error: std::error::Error + Send + 'static;

    /// Acquire a resource from the pool.
    ///
    /// # Errors
    ///
    /// Returns `Err` if there was a problem acquiring a resource from the pool.
    fn get(&'p self) -> Result<Self::Resource, Error<Self::Error>>;

    /// Instantly acquire a resource from the pool.
    ///
    /// # Errors
    ///
    /// Returns `Err` if blocking is required, if there was a problem acquiring a resource from the
    /// pool, or if this operation is not implemented for this pool type.
    fn try_get(&'p self) -> Result<Self::Resource, Error<Self::Error>> {
        Err(Error::NotImplemented)
    }

    /// Try to acquire a resource from the pool, waiting for a bounded time.
    ///
    /// # Errors
    ///
    /// Must return [`Error::AcquireTimeout`] if waiting time was exhaused.
    ///
    /// Returns [`Error::NotImplemented`] if this operation is not implemented for this pool type.
    fn get_timeout(&'p self, _timeout: Duration) -> Result<Self::Resource, Error<Self::Error>> {
        Err(Error::NotImplemented)
    }

    /// Get various internal pool counts and metrics.
    ///
    /// This is in turn used to update OpenTelemetry metrics.
    ///
    /// # Errors
    ///
    /// Returns `Err` if there was a problem collecting pool state, or [`Error::NotImplemented`] if
    /// state collection is not implemented for this pool type.
    fn get_state(&'p self) -> Result<PoolState, Error<Self::Error>> {
        Err(Error::NotImplemented)
    }
}
