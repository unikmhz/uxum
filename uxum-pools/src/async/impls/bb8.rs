use bb8::{ManageConnection, Pool, PooledConnection, RunError};

use crate::r#async::*;

#[async_trait::async_trait]
impl<'p, M> InstrumentablePool<'p> for Pool<M>
where
    M: ManageConnection,
    M::Error: std::error::Error,
{
    type Resource = PooledConnection<'p, M>;
    type Error = M::Error;

    async fn get(&'p self) -> Result<Self::Resource, Error<Self::Error>> {
        Pool::get(self).await.map_err(|e| match e {
            RunError::TimedOut => Error::AcquireTimeout,
            RunError::User(e) => Error::Pool(e),
        })
    }

    async fn get_timeout(
        &'p self,
        timeout: Duration,
    ) -> Result<Self::Resource, Error<Self::Error>> {
        match tokio::time::timeout(timeout, <Self as InstrumentablePool<'p>>::get(self)).await {
            Ok(res) => res,
            Err(_) => Err(Error::AcquireTimeout),
        }
    }

    fn get_state(&'p self) -> Result<PoolState, Error<Self::Error>> {
        let inner = Pool::state(self);
        Ok(PoolState {
            max_size: None,
            size: Some(inner.connections as usize),
            idle: Some(inner.idle_connections as usize),
            in_use: Some(inner.connections.saturating_sub(inner.idle_connections) as usize),
            min_idle: None,
            max_idle: None,
        })
    }
}
