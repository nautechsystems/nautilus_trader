//! Common message types.
//!
//! The [`Params`] type uses `IndexMap<String, Value>` for consistent ordering
//! and JSON value support.

// Re-export Params from the centralized params module
pub use crate::params::Params;
use crate::{UUID4, UnixNanos};

/// Represents different types of messages in the system.
#[derive(Debug, Clone)]
pub enum Message {
    /// A command message with an identifier and initialization timestamp.
    Command {
        /// The unique identifier for this command.
        id: UUID4,
        /// The initialization timestamp.
        ts_init: UnixNanos,
    },
    /// A document message with an identifier and initialization timestamp.
    Document {
        /// The unique identifier for this document.
        id: UUID4,
        /// The initialization timestamp.
        ts_init: UnixNanos,
    },
    /// An event message with identifiers and timestamps.
    Event {
        /// The unique identifier for this event.
        id: UUID4,
        /// The initialization timestamp.
        ts_init: UnixNanos,
        /// The event timestamp.
        ts_event: UnixNanos,
    },
    /// A request message with an identifier and initialization timestamp.
    Request {
        /// The unique identifier for this request.
        id: UUID4,
        /// The initialization timestamp.
        ts_init: UnixNanos,
    },
    /// A response message with identifiers, timestamps, and correlation.
    Response {
        /// The unique identifier for this response.
        id: UUID4,
        /// The initialization timestamp.
        ts_init: UnixNanos,
        /// The correlation identifier linking this response to a request.
        correlation_id: UUID4,
    },
}
