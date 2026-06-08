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

//! Bus capture adapter: the seam that converts dispatched bus messages into event store
//! entries.
//!
//! See `README.md` "Capture surface" and "Architecture" for the SPEC contract; the
//! [`BusCaptureAdapter`] implements the dispatch-boundary side of that contract by
//! consulting an [`EncoderRegistry`] allow-list and forwarding encoded entries to the
//! [`crate::EventStoreWriter`].

pub mod adapter;
pub mod builtins;
pub mod encoder;
pub mod registry;

pub use adapter::{BusCaptureAdapter, CaptureError};
pub use builtins::{
    PAYLOAD_TYPE_ACCOUNT_STATE, PAYLOAD_TYPE_FILL_REPORT, PAYLOAD_TYPE_ORDER_FILLED,
    PAYLOAD_TYPE_ORDER_STATUS_REPORT, PAYLOAD_TYPE_POSITION_STATUS_REPORT,
    PAYLOAD_TYPE_SUBMIT_ORDER, default_registry, encode_account_state, encode_fill_report,
    encode_order_filled, encode_order_status_report, encode_position_status_report,
    encode_submit_order, register_default,
};
pub use encoder::{Encode, EncodeError, EncodedPayload, TypedEncoder};
pub use registry::{EncoderRegistry, HeadersExtractor, TypedHeadersExtractor};
