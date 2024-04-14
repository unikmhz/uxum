use std::{
    hash::Hash,
    net::{IpAddr, SocketAddr},
};

use axum::extract::ConnectInfo;
use forwarded_header_value::{ForwardedHeaderValue, Identifier};
use http::{header::FORWARDED, HeaderMap, Request};
use thiserror::Error;

/// Error type returned by key extractors
#[derive(Clone, Debug, Error)]
#[error("Unable to extract rate-limiting key from request")]
pub struct ExtractionError;

pub(crate) trait KeyExtractor {
    type Key: Hash + Eq + Clone;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, ExtractionError>;
}

pub(crate) struct PeerIpKeyExtractor;

impl KeyExtractor for PeerIpKeyExtractor {
    type Key = IpAddr;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, ExtractionError> {
        maybe_connect_info(req).ok_or(ExtractionError)
    }
}

pub(crate) struct SmartIpKeyExtractor;

impl KeyExtractor for SmartIpKeyExtractor {
    type Key = IpAddr;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, ExtractionError> {
        let headers = req.headers();
        maybe_x_forwarded_for(headers)
            .or_else(|| maybe_x_real_ip(headers))
            .or_else(|| maybe_forwarded(headers))
            .or_else(|| maybe_connect_info(req))
            .ok_or(ExtractionError)
    }
}

// Following chunk was in part yoinked from tower_governor crate.
// See https://github.com/benwis/tower-governor/blob/main/src/key_extractor.rs

const X_REAL_IP: &str = "x-real-ip";
const X_FORWARDED_FOR: &str = "x-forwarded-for";

/// Tries to parse the `x-forwarded-for` header
fn maybe_x_forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get(X_FORWARDED_FOR)
        .and_then(|hv| hv.to_str().ok())
        .and_then(|hstr| {
            hstr.split(',')
                .find_map(|sp| sp.trim().parse::<IpAddr>().ok())
        })
}

/// Tries to parse the `x-real-ip` header
fn maybe_x_real_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get(X_REAL_IP)
        .and_then(|hv| hv.to_str().ok())
        .and_then(|hstr| hstr.parse::<IpAddr>().ok())
}

/// Tries to parse `forwarded` headers
fn maybe_forwarded(headers: &HeaderMap) -> Option<IpAddr> {
    headers.get_all(FORWARDED).iter().find_map(|hv| {
        hv.to_str()
            .ok()
            .and_then(|hstr| ForwardedHeaderValue::from_forwarded(hstr).ok())
            .and_then(|fhv| {
                fhv.iter()
                    .filter_map(|fs| fs.forwarded_for.as_ref())
                    .find_map(|ff| match ff {
                        Identifier::SocketAddr(addr) => Some(addr.ip()),
                        Identifier::IpAddr(ip) => Some(*ip),
                        _ => None,
                    })
            })
    })
}

/// Looks in `ConnectInfo` extension
fn maybe_connect_info<T>(req: &Request<T>) -> Option<IpAddr> {
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip())
}
