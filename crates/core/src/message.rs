// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
