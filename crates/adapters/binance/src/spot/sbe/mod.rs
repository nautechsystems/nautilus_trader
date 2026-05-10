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

//! Binance SBE (Simple Binary Encoding) codec implementations.
//!
//! This module contains:
//! - `cursor`: Re-export of shared cursor utilities from `nautilus_serialization::sbe`.
//! - `error`: Re-export of shared decode error types from `nautilus_serialization::sbe`.
//! - `generated`: Generated codecs for the Spot REST/WebSocket API (schema 3:3).
//! - `stream`: Hand-written codecs for market data streams (schema 1:0).
//!
//! The generated codecs come from Binance's official SBE schema using
//! Real Logic's SBE generator. The stream codecs are hand-written for the
//! 4 market data stream message types.

pub mod cursor;
pub mod error;
#[path = "generated/mod.rs"]
pub mod generated;
pub mod stream;

pub use cursor::SbeCursor;
pub use error::{MAX_GROUP_SIZE, SbeDecodeError};
pub use generated as spot;
pub use generated::{
    ReadBuf, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION, SbeErr, SbeResult,
    message_header_codec::MessageHeaderDecoder,
};
