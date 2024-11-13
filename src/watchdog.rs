//! Watchdog to monitor Tokio runtime responsiveness.

use std::{
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::{
    task::JoinHandle,
    time::{interval, MissedTickBehavior},
};
use tracing::{trace_span, Instrument};

/// Configuration for runtime watchdog.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct WatchdogConfig {
    /// Interval between watchdog updates.
    ///
    /// Default is 2 seconds.
    #[serde(default = "WatchdogConfig::default_interval", with = "humantime_serde")]
    interval: Duration,
    /// Duration since last update that is considered as a watchdog failure.
    ///
    /// Default is 7 seconds.
    #[serde(default = "WatchdogConfig::default_timeout", with = "humantime_serde")]
    timeout: Duration,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            interval: Self::default_interval(),
            timeout: Self::default_timeout(),
        }
    }
}

impl WatchdogConfig {
    /// Default value for [`Self::interval`].
    #[must_use]
    #[inline]
    fn default_interval() -> Duration {
        Duration::from_secs(2)
    }

    /// Default value for [`Self::timeout`].
    #[must_use]
    #[inline]
    fn default_timeout() -> Duration {
        Duration::from_secs(7)
    }
}

/// Runtime watchdog.
///
/// Runs every `config.interval` and updates its `last` value.
#[derive(Debug, Default)]
pub(crate) struct Watchdog {
    /// Runtime watchdog configuration.
    config: WatchdogConfig,
    /// Watchdog task handle.
    task: Option<JoinHandle<()>>,
    /// Value periodically updated by watchdog task.
    last: Arc<Mutex<Option<Instant>>>,
}

impl From<WatchdogConfig> for Watchdog {
    fn from(config: WatchdogConfig) -> Self {
        Self {
            config,
            task: None,
            last: Arc::new(Mutex::new(None)),
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl Watchdog {
    /// Create runtime watchdog future.
    fn watchdog_task(&self) -> impl Future<Output = ()> {
        let span = trace_span!("runtime_watchdog");
        let interval_dur = self.config.interval;
        let last_shared = self.last.clone();
        async move {
            let mut timer = interval(interval_dur);
            timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                // TODO: cancellation token?
                tokio::select! {
                    inst = timer.tick() => {
                        let mut last = last_shared.lock();
                        *last = Some(inst.into());
                    }
                }
            }
        }
        .instrument(span)
    }

    /// Start internal runtime watchdog task.
    pub(crate) fn start(&mut self) {
        if self.task.is_none() {
            self.task = Some(tokio::spawn(self.watchdog_task()));
        }
    }

    /// Check whether runtime watchdog has been tripped.
    #[must_use]
    pub(crate) fn is_alive(&self) -> bool {
        match self.last.lock().map(|l| l + self.config.timeout) {
            Some(th) => Instant::now() < th,
            None => true,
        }
    }
}
