use std::time::Duration;

use deadpool::managed::{Manager, Object, Pool, PoolError, Timeouts};

use crate::r#async::*;

#[async_trait::async_trait]
impl<'p, M, W> InstrumentablePool<'p> for Pool<M, W>
where
    M: Manager,
    M::Error: std::error::Error + 'static,
    W: From<Object<M>> + 'p,
{
    type Resource = W;
    type Error = M::Error;

    async fn get(&'p self) -> Result<Self::Resource, Error<Self::Error>> {
        Pool::get(self).await.map_err(|e| match e {
            // TODO: further specializee timeout types
            PoolError::Timeout(_) => Error::AcquireTimeout,
            PoolError::Backend(err) => Error::Pool(err),
            _ => Error::PoolExhausted,
        })
    }

    async fn get_timeout(
        &'p self,
        timeout: Duration,
    ) -> Result<Self::Resource, Error<Self::Error>> {
        let timeouts = Timeouts {
            wait: Some(timeout),
            create: Some(timeout),
            recycle: Some(timeout),
        };
        Pool::timeout_get(self, &timeouts)
            .await
            .map_err(|e| match e {
                // TODO: further specializee timeout types
                PoolError::Timeout(_) => Error::AcquireTimeout,
                PoolError::Backend(err) => Error::Pool(err),
                _ => Error::PoolExhausted,
            })
    }

    fn get_state(&'p self) -> Result<PoolState, Error<Self::Error>> {
        let inner = Pool::status(self);
        Ok(PoolState {
            max_size: Some(inner.max_size),
            size: Some(inner.size),
            idle: Some(inner.available),
            in_use: Some(inner.size.saturating_sub(inner.available)),
            min_idle: None,
            max_idle: None,
        })
    }
}
