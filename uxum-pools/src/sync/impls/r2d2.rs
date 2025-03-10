use std::time::Duration;

use r2d2::{Error as R2D2Error, ManageConnection, Pool, PooledConnection};

use crate::sync::*;

impl<'p, M: ManageConnection> InstrumentablePool<'p> for Pool<M> {
    type Resource = PooledConnection<M>;
    type Error = R2D2Error;

    fn get(&'p self) -> Result<Self::Resource, Error<Self::Error>> {
        // r2d2 error is untyped, and has no additional info apart from error string.
        Pool::get(self).map_err(Error::Pool)
    }

    fn try_get(&'p self) -> Result<Self::Resource, Error<Self::Error>> {
        Pool::try_get(self).ok_or(Error::WouldBlock)
    }

    fn get_timeout(&'p self, timeout: Duration) -> Result<Self::Resource, Error<Self::Error>> {
        // TODO: detect timeout condition ourselves?
        Pool::get_timeout(self, timeout).map_err(Error::Pool)
    }

    fn get_state(&'p self) -> Result<PoolState, Error<Self::Error>> {
        let inner = Pool::state(self);
        Ok(PoolState {
            max_size: Some(Pool::max_size(self) as usize),
            size: Some(inner.connections as usize),
            idle: Some(inner.idle_connections as usize),
            in_use: Some({
                if inner.connections > inner.idle_connections {
                    (inner.connections - inner.idle_connections) as usize
                } else {
                    0
                }
            }),
            min_idle: Pool::min_idle(self).map(|i| i as usize),
            max_idle: None,
        })
    }
}
