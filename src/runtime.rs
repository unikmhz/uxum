use std::{
    num::{NonZeroU32, NonZeroUsize},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::runtime::{Builder, Runtime};
use tracing::trace;

///
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RuntimeType {
    ///
    CurrentThread,
    ///
    #[default]
    MultiThread,
}

impl RuntimeType {
    ///
    pub fn builder(&self) -> Builder {
        match self {
            Self::CurrentThread => Builder::new_current_thread(),
            Self::MultiThread => Builder::new_multi_thread(),
        }
    }
}

///
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct RuntimeConfig {
    /// Type of runtime to use
    #[serde(default)]
    pub r#type: RuntimeType,
    ///
    ///
    /// Measured in runtime scheduler ticks.
    /// Current Tokio default is 61.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_interval: Option<NonZeroU32>,
    ///
    ///
    /// Measured in runtime scheduler ticks.
    /// Current Tokio default is 31 for current-thread scheduler, and computed dynamically for
    /// multi-thread scheduler. See [Tokio documentation] for details.
    ///
    /// [Tokio documentation]: tokio::runtime#multi-threaded-runtime-behavior-at-the-time-of-writing
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_unique_interval: Option<NonZeroU32>,
    ///
    ///
    /// Current Tokio default is 512.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_blocking_threads: Option<NonZeroUsize>,
    ///
    ///
    /// Current Tokio default is 1024.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_io_events_per_tick: Option<NonZeroUsize>,
    ///
    ///
    /// Current Tokio default is 10 seconds.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub thread_keep_alive: Option<Duration>,
    ///
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_name: Option<String>,
    ///
    ///
    /// Measured in bytes.
    /// Current Tokio default is 2 MiB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_stack_size: Option<NonZeroUsize>,
    ///
    /// Default is number of cores available at runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_threads: Option<NonZeroUsize>,
}

impl RuntimeConfig {
    ///
    pub fn builder(&self) -> Builder {
        let mut rb = self.r#type.builder();
        rb.enable_all();
        if let Some(interval) = self.event_interval {
            rb.event_interval(interval.get());
        }
        if let Some(interval) = self.global_unique_interval {
            rb.global_queue_interval(interval.get());
        }
        if let Some(max_blocking) = self.max_blocking_threads {
            rb.max_blocking_threads(max_blocking.get());
        }
        if let Some(max_events) = self.max_io_events_per_tick {
            rb.max_io_events_per_tick(max_events.get());
        }
        if let Some(duration) = self.thread_keep_alive {
            rb.thread_keep_alive(duration);
        }
        if let Some(size) = self.thread_stack_size {
            rb.thread_stack_size(size.get());
        }
        if let Some(num) = self.worker_threads {
            rb.worker_threads(num.get());
        }
        let prefix = self.thread_name.clone().unwrap_or("uxum-worker".into());
        rb.thread_name_fn(move || {
            static THREAD_COUNTER: AtomicUsize = AtomicUsize::new(0);
            let thread_id = THREAD_COUNTER.fetch_add(1, Ordering::Relaxed);
            format!("{prefix}-{thread_id}")
        })
        .on_thread_start(|| {
            trace!(thread_id = gettid::gettid(), "started runtime thread");
        })
        .on_thread_stop(|| {
            trace!(thread_id = gettid::gettid(), "stopping runtime thread");
        });
        // TODO: on_thread_stop
        rb
    }

    ///
    pub fn build(&self) -> Result<Runtime, std::io::Error> {
        self.builder().build()
    }
}
