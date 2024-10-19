use thiserror::Error;
use tokio::signal::unix;
use tracing::{info, warn};

use crate::errors::IoError;

/// Signal handling error.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SignalError {
    /// Unable to register signal handler.
    #[error("Unable to register signal handler: {0}")]
    Register(IoError),
}

/// Register signal handler.
///
/// # Errors
///
/// Returns `Err` if internal call to [`unix::signal`] fails.
fn register(kind: unix::SignalKind) -> Result<unix::Signal, SignalError> {
    unix::signal(kind).map_err(|err| SignalError::Register(err.into()))
}

/// Unified listener for all handled signals.
pub struct SignalStream {
    /// Signal listener for SIGTERM.
    sig_term: unix::Signal,
    /// Signal listener for SIGINT.
    sig_int: unix::Signal,
    /// Signal listener for SIGQUIT.
    sig_quit: unix::Signal,
    /// Signal listener for SIGHUP.
    sig_hup: unix::Signal,
    /// Signal listener for SIGUSR1.
    sig_usr1: unix::Signal,
    /// Signal listener for SIGUSR2.
    sig_usr2: unix::Signal,
}

/// Signal type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Signal {
    /// SIGTERM.
    Terminate,
    /// SIGINT.
    Interrupt,
    /// SIGQUIT.
    Quit,
    /// SIGHUP.
    HangUp,
    /// SIGUSR1.
    UserDefined1,
    /// SIGUSR2.
    UserDefined2,
}

impl Signal {
    /// Name of a signal, as written in UNIX manual pages.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Terminate => "SIGTERM",
            Self::Interrupt => "SIGINT",
            Self::Quit => "SIGQUIT",
            Self::HangUp => "SIGHUP",
            Self::UserDefined1 => "SIGUSR1",
            Self::UserDefined2 => "SIGUSR2",
        }
    }

    /// Whether a given signal should result in application shutting down.
    #[must_use]
    pub fn is_shutdown(&self) -> bool {
        matches!(self, Self::Terminate | Self::Interrupt | Self::Quit)
    }
}

impl SignalStream {
    /// Create new [`SignalStream`].
    ///
    /// Automatically registers all signal handlers.
    ///
    /// # Errors
    ///
    /// Returns `Err` if some signal handler failed to register.
    pub fn new() -> Result<Self, SignalError> {
        // TODO: cancellation token?
        let sig_term = register(unix::SignalKind::terminate())?;
        let sig_int = register(unix::SignalKind::interrupt())?;
        let sig_quit = register(unix::SignalKind::quit())?;
        let sig_hup = register(unix::SignalKind::hangup())?;
        let sig_usr1 = register(unix::SignalKind::user_defined1())?;
        let sig_usr2 = register(unix::SignalKind::user_defined2())?;

        Ok(Self {
            sig_term,
            sig_int,
            sig_quit,
            sig_hup,
            sig_usr1,
            sig_usr2,
        })
    }

    /// Wait for next received signal.
    ///
    /// # Errors
    ///
    /// Returns `Err` if some signal handler failed to register.
    pub async fn next(&mut self) -> Result<Signal, SignalError> {
        macro_rules! sig_recv {
            ($name:literal, $value:ident) => {
                info!(kind = $name, "received signal");
                return Ok(Signal::$value);
            };
        }
        macro_rules! sig_restart {
            ($name:literal, $sig:ident, $create:ident) => {
                warn!(kind = $name, "signal handler exited, restarting");
                self.$sig = register(unix::SignalKind::$create())?;
                continue;
            };
        }
        loop {
            tokio::select! {
                ret = self.sig_term.recv() => match ret {
                    Some(_) => { sig_recv!("SIGTERM", Terminate); }
                    None => { sig_restart!("SIGTERM", sig_term, terminate); }
                },
                ret = self.sig_int.recv() => match ret {
                    Some(_) => { sig_recv!("SIGINT", Interrupt); }
                    None => { sig_restart!("SIGINT", sig_int, interrupt); }
                },
                ret = self.sig_quit.recv() => match ret {
                    Some(_) => { sig_recv!("SIGQUIT", Quit); }
                    None => { sig_restart!("SIGQUIT", sig_quit, quit); }
                },
                ret = self.sig_hup.recv() => match ret {
                    Some(_) => { sig_recv!("SIGHUP", HangUp); }
                    None => { sig_restart!("SIGHUP", sig_hup, hangup); }
                },
                ret = self.sig_usr1.recv() => match ret {
                    Some(_) => { sig_recv!("SIGUSR1", UserDefined1); }
                    None => { sig_restart!("SIGUSR1", sig_usr1, user_defined1); }
                },
                ret = self.sig_usr2.recv() => match ret {
                    Some(_) => { sig_recv!("SIGUSR2", UserDefined2); }
                    None => { sig_restart!("SIGUSR2", sig_usr2, user_defined2); }
                },
            }
        }
    }
}
