use std::num::NonZeroUsize;

use serde::{Deserialize, Serialize};
use tower::buffer::BufferLayer;

/// Configuration for request buffering queue layer
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HandlerBufferConfig {
    /// Buffer queue depth
    pub queue: NonZeroUsize,
}

impl HandlerBufferConfig {
    /// Create layer for use in tower services
    pub fn make_layer<T>(&self) -> BufferLayer<T> {
        BufferLayer::new(self.queue.into())
    }
}
