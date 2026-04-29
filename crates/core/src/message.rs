// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_message_command_construction() {
        let msg = Message::Command {
            id: UUID4::new(),
            ts_init: UnixNanos::from(1_000),
        };
        assert!(matches!(msg, Message::Command { .. }));
    }

    #[rstest]
    fn test_message_document_construction() {
        let msg = Message::Document {
            id: UUID4::new(),
            ts_init: UnixNanos::from(2_000),
        };
        assert!(matches!(msg, Message::Document { .. }));
    }

    #[rstest]
    fn test_message_event_construction() {
        let msg = Message::Event {
            id: UUID4::new(),
            ts_init: UnixNanos::from(3_000),
            ts_event: UnixNanos::from(2_500),
        };
        assert!(matches!(msg, Message::Event { .. }));
    }

    #[rstest]
    fn test_message_request_construction() {
        let msg = Message::Request {
            id: UUID4::new(),
            ts_init: UnixNanos::from(4_000),
        };
        assert!(matches!(msg, Message::Request { .. }));
    }

    #[rstest]
    fn test_message_response_construction() {
        let id = UUID4::new();
        let correlation_id = UUID4::new();
        let msg = Message::Response {
            id,
            ts_init: UnixNanos::from(5_000),
            correlation_id,
        };
        assert!(matches!(msg, Message::Response { .. }));
    }

    #[rstest]
    #[expect(clippy::redundant_clone, reason = "Clone is the behavior under test")]
    fn test_message_clone() {
        let msg = Message::Command {
            id: UUID4::new(),
            ts_init: UnixNanos::from(1_000),
        };
        let cloned = msg.clone();
        assert!(matches!(cloned, Message::Command { .. }));
    }
}
