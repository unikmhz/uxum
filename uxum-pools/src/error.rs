//! Error types and error support code.

/// Generalized error type used by any instrumented pool.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Pool operation not supported.
    #[error("operation not supported")]
    NotImplemented,
    /// No available resources were found in the pool.
    #[error("pool is exhausted")]
    PoolExhausted,
    /// Call would block the thread, and non-blocking operation was requested.
    #[error("acquisition from pool would block execution")]
    WouldBlock,
    /// Resource acquisition took longer than the specified timeout.
    #[error("connection acquisition timeout")]
    AcquireTimeout,
    /// Pool implementation-specific error.
    // TODO: convert dyn to generic type
    #[error("pool error: {0}")]
    Pool(Box<dyn std::error::Error>),
}
