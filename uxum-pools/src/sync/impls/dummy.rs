use std::{convert::Infallible, time::Duration};

use crate::sync::*;

/// Dummy pool for testing purposes.
pub(crate) struct DummyPool;

/// Dummy resource for testing purposes.
pub(crate) struct DummyResource;

impl InstrumentablePool<'_> for DummyPool {
    type Resource = DummyResource;
    type Error = Infallible;

    fn get(&self) -> Result<Self::Resource, Error<Self::Error>> {
        std::thread::sleep(Duration::from_millis(10));
        Ok(DummyResource)
    }

    fn try_get(&self) -> Result<Self::Resource, Error<Self::Error>> {
        Ok(DummyResource)
    }

    fn get_timeout(&self, timeout: Duration) -> Result<Self::Resource, Error<Self::Error>> {
        std::thread::sleep(timeout);
        Ok(DummyResource)
    }

    fn get_state(&self) -> Result<PoolState, Error<Self::Error>> {
        Ok(PoolState::default())
    }
}
