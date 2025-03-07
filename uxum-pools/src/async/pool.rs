use std::{
    borrow::Cow,
    ops::{Deref, DerefMut},
    sync::Arc,
    time::{Duration, Instant},
};

use opentelemetry::KeyValue;
use parking_lot::Mutex;
use tracing::{debug_span, Instrument};

use crate::{
    error::Error,
    metrics::{pool_kv, Metrics, POOL_METRICS},
    r#async::InstrumentablePool,
    resource::InstrumentedResource,
};

/// Instrumented pool.
///
/// Automatically gathers pool-related metrics and provides relevant traces.
pub struct InstrumentedPool<P> {
    /// Pool label.
    label: [KeyValue; 1],
    /// Linked metrics storage.
    metrics: Arc<Metrics>,
    /// Time of last gathering of common pool metrics.
    last_gathered_at: Mutex<Instant>,
    /// Original resource pool.
    pool: P,
}

impl<P: for<'p> InstrumentablePool<'p> + Sync> InstrumentedPool<P> {
    /// Instrument provided resource pool.
    pub fn instrument<L: Into<Cow<'static, str>>>(
        label: Option<L>,
        pool: P,
    ) -> Result<Self, Error> {
        let label = pool_kv(label.map(Into::into));
        let metrics = POOL_METRICS.deref().clone();
        metrics.record_state(&label, pool.get_state()?);
        Ok(Self {
            label,
            metrics,
            last_gathered_at: Mutex::new(Instant::now()),
            pool,
        })
    }

    /// Manually update pool metrics.
    ///
    /// Normally you wouldn't need to call this directly, as metrics collection occurs
    /// automatically as you use the pool.
    pub fn update_metrics(&self) -> Result<(), Error> {
        // TODO: configure periodic state gathering interval
        const PROBE_INTERVAL: Duration = Duration::from_secs(15);
        let mut last_gathered_at = self.last_gathered_at.lock();
        if last_gathered_at.elapsed() > PROBE_INTERVAL {
            *last_gathered_at = Instant::now();
            drop(last_gathered_at);
            self.metrics
                .record_state(&self.label, self.pool.get_state()?);
        }
        Ok(())
    }

    /// Internal method to record metrics after resource acquisition.
    #[inline]
    fn measure_acquire(&self, before: Instant) -> Result<(), Error> {
        self.metrics
            .wait_time
            .record(before.elapsed().as_secs_f64(), &self.label);
        self.update_metrics()
    }

    /// Acquire instrumented resource from resource pool.
    pub async fn get(
        &self,
    ) -> Result<InstrumentedResource<<P as InstrumentablePool<'_>>::Resource>, Error> {
        let now = Instant::now();
        let span = debug_span!("pool_acquire", name = self.label[0].value.as_str().as_ref());
        let resource = self.pool.get().instrument(span).await?;
        self.measure_acquire(now)?;
        Ok(InstrumentedResource::new(
            self.metrics.clone(),
            self.label.clone(),
            resource,
        ))
    }

    /// Instantly acquire an instrumented resource from the pool.
    ///
    /// # Errors
    ///
    /// Returns `Err` if blocking is required, or if this operation is not implemented for this
    /// pool type.
    pub fn try_get(
        &self,
    ) -> Result<InstrumentedResource<<P as InstrumentablePool<'_>>::Resource>, Error> {
        let now = Instant::now();
        let span = debug_span!(
            "pool_try_acquire",
            name = self.label[0].value.as_str().as_ref()
        )
        .entered();
        let resource = self.pool.try_get()?;
        drop(span);
        self.measure_acquire(now)?;
        Ok(InstrumentedResource::new(
            self.metrics.clone(),
            self.label.clone(),
            resource,
        ))
    }

    /// Try to acquire an instrumented resource from the pool, waiting for a bounded time.
    ///
    /// # Errors
    ///
    /// Must return [`Error::AcquireTimeout`] if waiting time was exhaused.
    ///
    /// Returns [`Error::NotImplemented`] if this operation is not implemented for this pool type.
    pub async fn get_timeout(
        &self,
        timeout: Duration,
    ) -> Result<InstrumentedResource<<P as InstrumentablePool<'_>>::Resource>, Error> {
        let now = Instant::now();
        let span = debug_span!(
            "pool_timed_acquire",
            name = self.label[0].value.as_str().as_ref()
        );
        let resource = self.pool.get_timeout(timeout).instrument(span).await?;
        self.measure_acquire(now)?;
        Ok(InstrumentedResource::new(
            self.metrics.clone(),
            self.label.clone(),
            resource,
        ))
    }
}

impl<P> Deref for InstrumentedPool<P> {
    type Target = P;

    fn deref(&self) -> &Self::Target {
        &self.pool
    }
}

impl<P> DerefMut for InstrumentedPool<P> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.pool
    }
}

impl<P> AsRef<P> for InstrumentedPool<P> {
    fn as_ref(&self) -> &P {
        &self.pool
    }
}

impl<P> AsMut<P> for InstrumentedPool<P> {
    fn as_mut(&mut self) -> &mut P {
        &mut self.pool
    }
}

impl<P: Clone> Clone for InstrumentedPool<P> {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            metrics: self.metrics.clone(),
            last_gathered_at: Mutex::new(*self.last_gathered_at.lock()),
            pool: self.pool.clone(),
        }
    }
}

// TODO: Debug, Display
