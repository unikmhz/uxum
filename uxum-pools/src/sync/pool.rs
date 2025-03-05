use std::{borrow::Cow, ops::{Deref, DerefMut}, sync::Arc, time::{Duration, Instant}};

use opentelemetry::KeyValue;
use parking_lot::Mutex;
use tracing::debug_span;

use crate::{error::Error, metrics::{Metrics, POOL_METRICS, pool_kv}, sync::InstrumentablePool};

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

impl<P: InstrumentablePool> InstrumentedPool<P> {
    /// Instrument provided resource pool.
    pub fn instrument<L: Into<Cow<'static, str>>>(label: Option<L>, pool: P) -> Result<Self, Error> {
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

    /// Get instrumented resource from resource pool.
    pub fn get(&self) -> Result<InstrumentedResource<P::Resource>, Error> {
        let metrics = self.metrics.clone();
        let now = Instant::now();
        let label = self.label.clone();
        let span = debug_span!("pool_acquire", name = label[0].value.as_str().as_ref()).entered();
        let resource = self.pool.get()?;
        drop(span);
        metrics.wait_time.record(now.elapsed().as_secs_f64(), &label);
        {
            // TODO: configure periodic state gathering interval
            const PROBE_INTERVAL: Duration = Duration::from_secs(15);
            let mut last_gathered_at = self.last_gathered_at.lock();
            if last_gathered_at.elapsed() > PROBE_INTERVAL {
                *last_gathered_at = Instant::now();
                drop(last_gathered_at);
                metrics.record_state(&label, self.pool.get_state()?);
            }
        }
        Ok(InstrumentedResource {
            metrics: self.metrics.clone(),
            label,
            // This is different from time used in wait_time metric.
            time: Instant::now(),
            resource,
        })
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

/// Instrumented resource.
pub struct InstrumentedResource<R> {
    /// Linked metrics storage.
    metrics: Arc<Metrics>,
    /// Premade label used to record metrics.
    label: [KeyValue; 1],
    /// Retrieval time.
    time: Instant,
    /// Original resource.
    resource: R,
}

impl<R> Deref for InstrumentedResource<R> {
    type Target = R;

    fn deref(&self) -> &Self::Target {
        &self.resource
    }
}

impl<R> DerefMut for InstrumentedResource<R> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.resource
    }
}

impl<R> AsRef<R> for InstrumentedResource<R> {
    fn as_ref(&self) -> &R {
        &self.resource
    }
}

impl<R> AsMut<R> for InstrumentedResource<R> {
    fn as_mut(&mut self) -> &mut R {
        &mut self.resource
    }
}

impl<R: Clone> Clone for InstrumentedResource<R> {
    fn clone(&self) -> Self {
        Self {
            metrics: self.metrics.clone(),
            label: self.label.clone(),
            time: self.time,
            resource: self.resource.clone(),
        }
    }
}

// TODO: Debug, Display

impl<R> Drop for InstrumentedResource<R> {
    fn drop(&mut self) {
        // Record time spent outside the pool.
        self.metrics.use_time.record(self.time.elapsed().as_secs_f64(), &self.label);
    }
}
