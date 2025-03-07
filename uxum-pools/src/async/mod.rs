//! Support for asynchronous pools.

mod impls;
mod pool;

use std::time::Duration;

pub use crate::{error::Error, metrics::PoolState, r#async::pool::InstrumentedPool};

/// Pool objects implementing this trait can be instrumented.
#[async_trait::async_trait]
pub trait InstrumentablePool<'p> {
    /// Resource type contained in the pool.
    type Resource;

    /// Acquire a resource from the pool.
    async fn get(&'p self) -> Result<Self::Resource, Error>;

    /// Instantly acquire a resource from the pool.
    ///
    /// # Errors
    ///
    /// Returns `Err` if blocking is required, or if this operation is not implemented for this
    /// pool type.
    fn try_get(&'p self) -> Result<Self::Resource, Error> {
        Err(Error::NotImplemented)
    }

    /// Try to acquire a resource from the pool, waiting for a bounded time.
    ///
    /// # Errors
    ///
    /// Must return [`Error::AcquireTimeout`] if waiting time was exhaused.
    ///
    /// Returns [`Error::NotImplemented`] if this operation is not implemented for this pool type.
    async fn get_timeout(&'p self, _timeout: Duration) -> Result<Self::Resource, Error> {
        Err(Error::NotImplemented)
    }

    /// Get various internal pool counts and metrics.
    ///
    /// This is in turn used to update OpenTelemetry metrics.
    fn get_state(&'p self) -> Result<PoolState, Error> {
        Err(Error::NotImplemented)
    }
}
