//! Wrappers for contained resources and operations over them.

use std::{
    borrow::Cow,
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    sync::Arc,
    task::{ready, Context, Poll},
    time::Instant,
};

use opentelemetry::{Key, KeyValue};
use pin_project::pin_project;
use tracing::{debug_span, instrument::Instrumented, Instrument, Span};

use crate::metrics::Metrics;

const KEY_OP: Key = Key::from_static_str("db.operation.name");

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

    /// Create a guard for running an operation.
    pub fn op<O: Into<Cow<'static, str>>>(&self, op_name: O) -> InstrumentedOperation {
        let name: Cow<'static, str> = op_name.into();
        let span = debug_span!("op", "db.operation.name" = name.to_string());
        let labels = [self.label[0].clone(), KeyValue::new(KEY_OP, name)];
        InstrumentedOperation {
            span,
            metrics: self.metrics.clone(),
            labels,
            time: Instant::now(),
        }
    }

    /// Instrument and measure execution of a closure.
    #[inline]
    pub fn wrap<O, F, T>(self, op_name: O, func: F) -> T
    where
        O: Into<Cow<'static, str>>,
        F: FnOnce(Self) -> T,
    {
        let op = self.op(op_name);
        op.span.in_scope(|| func(self))
    }

    /// Instrument and measure execution of a generated future.
    pub fn wrap_async<'a, T, O, F, Fut>(&'a self, op_name: O, func: F) -> InstrumentedFuture<Fut>
    where
        O: Into<Cow<'static, str>>,
        F: FnOnce(&'a R) -> Fut,
        Fut: Future<Output = T>,
    {
        let op = self.op(op_name);
        InstrumentedFuture {
            inner: func(&self.resource).instrument(op.span.clone()),
            op: Some(op),
        }
    }

    /// Instrument and measure execution of a generated future.
    pub fn wrap_async_mut<'a, T, O, F, Fut>(
        &'a mut self,
        op_name: O,
        func: F,
    ) -> InstrumentedFuture<Fut>
    where
        O: Into<Cow<'static, str>>,
        F: FnOnce(&'a mut R) -> Fut,
        Fut: Future<Output = T>,
    {
        let op = self.op(op_name);
        InstrumentedFuture {
            inner: func(&mut self.resource).instrument(op.span.clone()),
            op: Some(op),
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

/// Instrumented operation on a resource.
pub struct InstrumentedOperation {
    /// Span to wrap operation in.
    span: Span,
    /// Linked metrics storage.
    metrics: Arc<Metrics>,
    /// Premade labels used to record metrics.
    labels: [KeyValue; 2],
    /// Operation start time.
    time: Instant,
}

impl Drop for InstrumentedOperation {
    fn drop(&mut self) {
        // Record time spent performing this operation.
        self.metrics
            .op_duration
            .record(self.time.elapsed().as_secs_f64(), &self.labels);
    }
}

/// Wrapping future providing operation instrumentation.
#[pin_project]
#[non_exhaustive]
pub struct InstrumentedFuture<F> {
    /// Inner future.
    #[pin]
    inner: Instrumented<F>,
    /// Instrumentation.
    op: Option<InstrumentedOperation>,
}

impl<F: Future> Future for InstrumentedFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let ret = ready!(this.inner.poll(cx));
        // Force drop execution.
        // TODO: maybe use ManuallyDrop here (unsafe).
        this.op.take();
        Poll::Ready(ret)
    }
}
