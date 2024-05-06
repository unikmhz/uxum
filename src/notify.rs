use std::{future::Future, time::Duration};

use libsystemd::daemon::{self, NotifyState};
use tokio::time::MissedTickBehavior;
use tracing::{error, info, trace, trace_span, warn, Instrument};

/// Interact with service supervisor
///
/// Currently only detects and supports running under `systemd`.
/// If not run under `systemd`, then using this struct is a no-op.
pub struct ServiceNotifier {
    has_systemd: bool,
}

impl Default for ServiceNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceNotifier {
    /// Create new service notifier
    #[must_use]
    pub fn new() -> Self {
        Self {
            has_systemd: daemon::booted(),
        }
    }

    /// Get requested watchdog interval
    ///
    /// Returns [`None`] if watchdog is not enabled or not running under `systemd`.
    #[must_use]
    fn watchdog_interval(&self) -> Option<Duration> {
        match self.has_systemd {
            true => daemon::watchdog_enabled(true).map(|dur| dur / 2),
            false => None,
        }
    }

    /// Notify that the service is ready to accept requests
    pub fn on_ready(&self) {
        if !self.has_systemd {
            return;
        }
        match daemon::notify(false, &[NotifyState::Ready]) {
            Ok(true) => info!("supervisor notified: service ready"),
            Ok(false) => warn!("supervisor unavailable: service ready"),
            Err(err) => error!(%err, "supervisor notification error"),
        }
    }

    /// Notify that the service is reloading itself
    pub fn on_reload(&self) {
        if !self.has_systemd {
            return;
        }
        match daemon::notify(false, &[NotifyState::Reloading]) {
            Ok(true) => info!("supervisor notified: service reloading"),
            Ok(false) => warn!("supervisor unavailable: service reloading"),
            Err(err) => error!(%err, "supervisor notification error"),
        }
    }

    /// Notify that the service is stopping
    pub fn on_shutdown(&self) {
        if !self.has_systemd {
            return;
        }
        match daemon::notify(false, &[NotifyState::Stopping]) {
            Ok(true) => info!("supervisor notified: service stopping"),
            Ok(false) => warn!("supervisor unavailable: service stopping"),
            Err(err) => error!(%err, "supervisor notification error"),
        }
    }

    /// Get watchdog task
    ///
    /// Generates an eternally waiting future if watchdog is not enabled or not running under `systemd`.
    pub fn watchdog_task(&self) -> impl Future<Output = ()> {
        let span = trace_span!("systemd_watchdog");
        let interval_time = self.watchdog_interval();
        async move {
            match interval_time {
                None => futures::pending!(),
                Some(int) => {
                    let mut interval = tokio::time::interval(int);
                    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
                    loop {
                        // TODO: cancellation token?
                        tokio::select! {
                            _ = interval.tick() => match daemon::notify(false, &[NotifyState::Watchdog]) {
                                Ok(true) => trace!("supervisor notified: watchdog tick"),
                                Ok(false) => warn!("supervisor unavailable: watchdog tick"),
                                Err(err) => error!(%err, "watchdog notification error"),
                            }
                        }
                    }
                }
            }
        }
        .instrument(span)
    }
}
