//! Network error types.

use thiserror::Error;

/// Error type for send operations in network clients.
#[derive(Error, Debug)]
pub enum SendError {
    /// The client has been closed or is disconnecting.
    #[error("send failed: client closed or disconnecting")]
    Closed,
    /// Timed out waiting for the client to become active.
    #[error("send failed: timeout waiting for active state")]
    Timeout,
    /// Failed to send because the writer channel is closed.
    #[error("send failed: broken pipe ({0})")]
    BrokenPipe(String),
}
