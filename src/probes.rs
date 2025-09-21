//! Service probe and maintenance mode API endpoints.

use std::{
    ops::Deref,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use axum::{
    error_handling::HandleErrorLayer,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{self, Router},
};
use serde::{Deserialize, Serialize};
use tower::ServiceBuilder;
use tracing::{debug_span, info};

use crate::{
    auth::{AuthExtractor, AuthLayer, AuthProvider},
    builder::app::error_handler,
    watchdog::{Watchdog, WatchdogConfig},
};

/// Configuration for service probes and management mode API.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct ProbeConfig {
    /// URL path for readiness probe.
    #[serde(default = "ProbeConfig::default_readiness_path")]
    readiness_path: String,
    /// URL path for liveness probe.
    #[serde(default = "ProbeConfig::default_liveness_path")]
    liveness_path: String,
    /// URL path to enable maintenance mode.
    #[serde(default = "ProbeConfig::default_maintenance_on_path")]
    maintenance_on_path: String,
    /// URL path to disable maintenance mode.
    #[serde(default = "ProbeConfig::default_maintenance_off_path")]
    maintenance_off_path: String,
    /// Runtime watchdog configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    watchdog: Option<WatchdogConfig>,
    #[serde(default)]
    start_in_maintenance: bool,
}

impl Default for ProbeConfig {
    fn default() -> Self {
        Self {
            readiness_path: Self::default_readiness_path(),
            liveness_path: Self::default_liveness_path(),
            maintenance_on_path: Self::default_maintenance_on_path(),
            maintenance_off_path: Self::default_maintenance_off_path(),
            watchdog: Some(WatchdogConfig::default()),
            start_in_maintenance: false,
        }
    }
}

impl ProbeConfig {
    /// Default value for [`Self::readiness_path`].
    #[must_use]
    #[inline]
    fn default_readiness_path() -> String {
        "/probe/ready".into()
    }

    /// Default value for [`Self::liveness_path`].
    #[must_use]
    #[inline]
    fn default_liveness_path() -> String {
        "/probe/live".into()
    }

    /// Default value for [`Self::maintenance_on_path`].
    #[must_use]
    #[inline]
    fn default_maintenance_on_path() -> String {
        "/maintenance/on".into()
    }

    /// Default value for [`Self::maintenance_off_path`].
    #[must_use]
    #[inline]
    fn default_maintenance_off_path() -> String {
        "/maintenance/off".into()
    }

    /// Build Axum router containing all probe and maintenance methods.
    pub fn build_router(
        &self,
        auth_provider: Box<dyn AuthProvider>,
        auth_extractor: Box<dyn AuthExtractor>,
    ) -> Router {
        // TODO: add toggle for probes, and possibly for maintenance mode.
        let _span = debug_span!("build_probes").entered();
        let state = ProbeState::new(self.start_in_maintenance, self.watchdog.as_ref());
        Router::new()
            .route(&self.readiness_path, routing::get(readiness_probe))
            .route(&self.liveness_path, routing::get(liveness_probe))
            .merge(
                Router::new()
                    .route(&self.maintenance_on_path, routing::post(maintenance_on))
                    .route(&self.maintenance_off_path, routing::post(maintenance_off))
                    .layer(
                        ServiceBuilder::new()
                            .layer(HandleErrorLayer::new(error_handler))
                            .layer(AuthLayer::new(
                                &["maintenance"],
                                auth_provider,
                                auth_extractor,
                            )),
                    ),
            )
            .with_state(state)
    }
}

/// Shared state for probes and maintenance mode API.
#[derive(Clone)]
pub struct ProbeState(Arc<ProbeStateInner>);

impl Deref for ProbeState {
    type Target = ProbeStateInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Default for ProbeState {
    fn default() -> Self {
        Self(Arc::new(ProbeStateInner {
            in_maintenance: AtomicBool::new(true),
            watchdog: None,
        }))
    }
}

impl ProbeState {
    /// Create new [`ProbeState`] with optional [`WatchdogConfig`].
    #[must_use]
    pub fn new(in_maint: bool, watchdog: Option<&WatchdogConfig>) -> Self {
        Self(Arc::new(ProbeStateInner {
            in_maintenance: AtomicBool::new(in_maint),
            watchdog: watchdog.map(|wc| {
                let mut watchdog: Watchdog = wc.clone().into();
                watchdog.start();
                watchdog
            }),
        }))
    }
}

/// Inner struct for probes/maintenance shared state.
pub struct ProbeStateInner {
    /// Maintenance mode flag.
    in_maintenance: AtomicBool,
    /// Optional runtime watchdog for use in liveness probes.
    watchdog: Option<Watchdog>,
}

/// Readiness probe handler.
///
/// For use in k8s-like deployments.
async fn readiness_probe(state: State<ProbeState>) -> impl IntoResponse {
    match state.in_maintenance.load(Ordering::Relaxed) {
        true => StatusCode::SERVICE_UNAVAILABLE,
        false => StatusCode::OK,
    }
}

/// Liveness probe handler.
///
/// For use in k8s-like deployments.
async fn liveness_probe(state: State<ProbeState>) -> impl IntoResponse {
    match &state.watchdog {
        Some(watchdog) => match watchdog.is_alive() {
            true => StatusCode::OK,
            false => StatusCode::SERVICE_UNAVAILABLE,
        },
        None => StatusCode::OK,
    }
}

/// Enable maintenance mode.
async fn maintenance_on(state: State<ProbeState>) -> impl IntoResponse {
    if !state.in_maintenance.swap(true, Ordering::Relaxed) {
        info!("maintenance mode enabled");
    }
    StatusCode::OK
}

/// Disable maintenance mode.
async fn maintenance_off(state: State<ProbeState>) -> impl IntoResponse {
    if state.in_maintenance.swap(false, Ordering::Relaxed) {
        info!("maintenance mode disabled");
    }
    StatusCode::OK
}
