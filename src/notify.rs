use std::{future::Future, time::Duration};

use libsystemd::daemon::{self, NotifyState};
use tokio::time::MissedTickBehavior;
use tracing::{error, info, trace, warn};

pub struct ServiceNotifier {
    has_systemd: bool,
}

impl Default for ServiceNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceNotifier {
    pub fn new() -> Self {
        Self {
            has_systemd: daemon::booted(),
        }
    }

    fn watchdog_interval(&self) -> Option<Duration> {
        match self.has_systemd {
            true => daemon::watchdog_enabled(true).map(|dur| dur / 2),
            false => None,
        }
    }

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

    pub fn watchdog_task(&self) -> impl Future<Output = ()> {
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
    }
}
