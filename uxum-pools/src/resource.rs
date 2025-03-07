//!

use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
    time::Instant,
};

use opentelemetry::KeyValue;

use crate::metrics::Metrics;

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

impl<R> InstrumentedResource<R> {
    /// Bundle resource, metrics container and identifier for the originating pool.
    pub(crate) fn new(metrics: Arc<Metrics>, label: [KeyValue; 1], resource: R) -> Self {
        Self {
            metrics,
            label,
            // This is different from time used in wait_time metric.
            time: Instant::now(),
            resource,
        }
    }
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
        self.metrics
            .use_time
            .record(self.time.elapsed().as_secs_f64(), &self.label);
    }
}
