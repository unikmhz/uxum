use std::time::Duration;

use crate::sync::*;

/// Dummy pool for testing purposes.
pub struct DummyPool;

/// Dummy resource for testing purposes.
pub struct DummyResource;

impl InstrumentablePool for DummyPool {
    type Resource = DummyResource;

    fn get(&self) -> Result<Self::Resource, Error> {
        std::thread::sleep(Duration::from_millis(10));
        Ok(DummyResource)
    }

    fn try_get(&self) -> Result<Self::Resource, Error> {
        Ok(DummyResource)
    }

    fn get_timeout(&self, timeout: Duration) -> Result<Self::Resource, Error> {
        std::thread::sleep(timeout);
        Ok(DummyResource)
    }

    fn get_state(&self) -> Result<PoolState, Error> {
        Ok(PoolState::default())
    }
}
