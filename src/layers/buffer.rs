use serde::{Deserialize, Serialize};
use tower::buffer::BufferLayer;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HandlerBufferConfig {
    /// Buffer queue depth.
    pub queue: usize,
}

impl HandlerBufferConfig {
    pub fn make_layer<T>(&self) -> BufferLayer<T> {
        BufferLayer::new(self.queue)
    }
}
