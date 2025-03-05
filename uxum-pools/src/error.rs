#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("operation not supported")]
    NotImplemented,
    #[error("pool is exhausted")]
    PoolExhausted,
    #[error("acquisition from pool would block execution")]
    WouldBlock,
    #[error("connection acquisition timeout")]
    AcquireTimeout,
    #[error("pool error: {0}")]
    Pool(Box<dyn std::error::Error>),
}
