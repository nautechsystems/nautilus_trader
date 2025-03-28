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

//! An `OrderBookDepth10` aggregated top-of-book data type with a fixed depth of 10 levels per side.

use std::{collections::HashMap, fmt::Display};

use indexmap::IndexMap;
use nautilus_core::{UnixNanos, serialization::Serializable};
use serde::{Deserialize, Serialize};

use super::{GetTsInit, order::BookOrder};
use crate::{identifiers::InstrumentId, types::fixed::FIXED_SIZE_BINARY};

pub const DEPTH10_LEN: usize = 10;

/// Represents a aggregated order book update with a fixed depth of 10 levels per side.
///
/// This structure is specifically designed for scenarios where a snapshot of the top 10 bid and
/// ask levels in an order book is needed. It differs from `OrderBookDelta` or `OrderBookDeltas`
/// in its fixed-depth nature and is optimized for cases where a full depth representation is not
/// required or practical.
///
/// Note: This type is not compatible with `OrderBookDelta` or `OrderBookDeltas` due to
/// its specialized structure and limited depth use case.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderBookDepth10 {
    /// The instrument ID for the book.
    pub instrument_id: InstrumentId,
    /// The bid orders for the depth update.
    pub bids: [BookOrder; DEPTH10_LEN],
    /// The ask orders for the depth update.
    pub asks: [BookOrder; DEPTH10_LEN],
    /// The count of bid orders per level for the depth update.
    pub bid_counts: [u32; DEPTH10_LEN],
    /// The count of ask orders per level for the depth update.
    pub ask_counts: [u32; DEPTH10_LEN],
    /// The record flags bit field, indicating event end and data information.
    pub flags: u8,
    /// The message sequence number assigned at the venue.
    pub sequence: u64,
    /// UNIX timestamp (nanoseconds) when the book event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the struct was initialized.
    pub ts_init: UnixNanos,
}

impl OrderBookDepth10 {
    /// Creates a new [`OrderBookDepth10`] instance.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        bids: [BookOrder; DEPTH10_LEN],
        asks: [BookOrder; DEPTH10_LEN],
        bid_counts: [u32; DEPTH10_LEN],
        ask_counts: [u32; DEPTH10_LEN],
        flags: u8,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            flags,
            sequence,
            ts_event,
            ts_init,
        }
    }

    /// Returns the metadata for the type, for use with serialization formats.
    #[must_use]
    pub fn get_metadata(
        instrument_id: &InstrumentId,
        price_precision: u8,
        size_precision: u8,
    ) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert("instrument_id".to_string(), instrument_id.to_string());
        metadata.insert("price_precision".to_string(), price_precision.to_string());
        metadata.insert("size_precision".to_string(), size_precision.to_string());
        metadata
    }

    /// Returns the field map for the type, for use with Arrow schemas.
    #[must_use]
    pub fn get_fields() -> IndexMap<String, String> {
        let mut metadata = IndexMap::new();
        metadata.insert("bid_price_0".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_1".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_2".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_3".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_4".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_5".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_6".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_7".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_8".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_9".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_0".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_1".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_2".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_3".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_4".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_5".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_6".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_7".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_8".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_9".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_0".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_1".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_2".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_3".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_4".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_5".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_6".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_7".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_8".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_9".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_0".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_1".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_2".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_3".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_4".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_5".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_6".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_7".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_8".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_9".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_count_0".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_1".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_2".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_3".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_4".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_5".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_6".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_7".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_8".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_9".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_0".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_1".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_2".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_3".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_4".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_5".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_6".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_7".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_8".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_9".to_string(), "UInt32".to_string());
        metadata.insert("flags".to_string(), "UInt8".to_string());
        metadata.insert("sequence".to_string(), "UInt64".to_string());
        metadata.insert("ts_event".to_string(), "UInt64".to_string());
        metadata.insert("ts_init".to_string(), "UInt64".to_string());
        metadata
    }
}

// TODO: Exact format for Debug and Display TBD
impl Display for OrderBookDepth10 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},flags={},sequence={},ts_event={},ts_init={}",
            self.instrument_id, self.flags, self.sequence, self.ts_event, self.ts_init
        )
    }
}

impl Serializable for OrderBookDepth10 {}

impl GetTsInit for OrderBookDepth10 {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::data::stubs::*;

    #[rstest]
    fn test_new(stub_depth10: OrderBookDepth10) {
        let depth = stub_depth10;
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let flags = 0;
        let sequence = 0;
        let ts_event = 1;
        let ts_init = 2;

        assert_eq!(depth.instrument_id, instrument_id);
        assert_eq!(depth.bids.len(), 10);
        assert_eq!(depth.asks.len(), 10);
        assert_eq!(depth.asks[9].price.as_f64(), 109.0);
        assert_eq!(depth.asks[0].price.as_f64(), 100.0);
        assert_eq!(depth.bids[0].price.as_f64(), 99.0);
        assert_eq!(depth.bids[9].price.as_f64(), 90.0);
        assert_eq!(depth.bid_counts.len(), 10);
        assert_eq!(depth.ask_counts.len(), 10);
        assert_eq!(depth.bid_counts[0], 1);
        assert_eq!(depth.ask_counts[0], 1);
        assert_eq!(depth.flags, flags);
        assert_eq!(depth.sequence, sequence);
        assert_eq!(depth.ts_event, ts_event);
        assert_eq!(depth.ts_init, ts_init);
    }

    // TODO: Exact format for Debug and Display TBD
    #[rstest]
    fn test_display(stub_depth10: OrderBookDepth10) {
        let depth = stub_depth10;
        assert_eq!(
            format!("{depth}"),
            "AAPL.XNAS,flags=0,sequence=0,ts_event=1,ts_init=2".to_string()
        );
    }
}
