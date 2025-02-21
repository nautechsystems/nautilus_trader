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

#[derive(Debug, Clone)]
pub enum Message {
    Command {
        id: UUID4,
        ts_init: UnixNanos,
    },
    Document {
        id: UUID4,
        ts_init: UnixNanos,
    },
    Event {
        id: UUID4,
        ts_init: UnixNanos,
        ts_event: UnixNanos,
    },
    Request {
        id: UUID4,
        ts_init: UnixNanos,
    },
    Response {
        id: UUID4,
        ts_init: UnixNanos,
        correlation_id: UUID4,
    },
}
