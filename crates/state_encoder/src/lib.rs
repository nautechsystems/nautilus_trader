//! State encoder for KuaaMU Quant Engine.
//!
//! Encodes Nautilus internal events into a shared-memory `ContextWindow`
//! that agents can read via mmap with zero serialization overhead.

pub mod context_window;
pub mod encoder;
pub mod shared_buffer;

pub use context_window::{ContextWindow, EventToken, Greeks};
pub use encoder::StateEncoder;
pub use shared_buffer::SharedStateBuffer;
