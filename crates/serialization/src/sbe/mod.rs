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

//! Generic SBE (Simple Binary Encoding) codec utilities.
//!
//! This module provides:
//! - [`SbeCursor`]: Zero-copy byte cursor with typed little-endian readers.
//! - [`SbeWriter`]: Pre-sized byte writer with typed little-endian writers.
//! - [`SbeEncodeError`] and [`SbeDecodeError`]: Common SBE codec errors.
//! - [`market`]: Hand-written Nautilus market-data SBE codecs.
//! - [`GroupSizeEncoding`] and [`GroupSize16Encoding`]: Group header decoders.
//! - [`decode_var_string8`]: varString8 decoder helper.

pub mod cursor;
pub mod error;
pub mod market;
pub mod primitives;
pub mod writer;

pub use cursor::SbeCursor;
pub use error::{MAX_GROUP_SIZE, SbeDecodeError, SbeEncodeError};
pub use market::{DataAny, FromSbe, FromSbeReuse, ToSbe};
pub use primitives::{GroupSize16Encoding, GroupSizeEncoding, decode_var_string8};
pub use writer::SbeWriter;
