use thiserror::Error;

#[derive(Debug, Error)]
pub enum ServerBuilderError {
    #[error("Unable to bind listener: {0}")]
    ListenerBind(std::io::Error),
    #[error("Unable to extract local address: {0}")]
    ListenerLocalAddr(hyper::Error),
    #[error("Neither HTTP/1 nor HTTP/2 are enabled")]
    NoProtocolsEnabled,
}
