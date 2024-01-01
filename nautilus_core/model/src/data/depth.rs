// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::fmt::{Display, Formatter};

use nautilus_core::{serialization::Serializable, time::UnixNanos};
use pyo3::prelude::*;
use serde::{Deserialize, Serialize};

use super::order::BookOrder;
use crate::identifiers::instrument_id::InstrumentId;

pub const DEPTH_10_LEN: usize = 10;

/// Represents a self-contained order book update with a fixed depth of 10 levels per side.
///
/// This struct is specifically designed for scenarios where a snapshot of the top 10 bid and
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
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderBookDepth10 {
    /// The instrument ID for the book.
    pub instrument_id: InstrumentId,
    /// The bid orders for the depth update.
    pub bids: [BookOrder; DEPTH_10_LEN],
    /// The ask orders for the depth update.
    pub asks: [BookOrder; DEPTH_10_LEN],
    /// A combination of packet end with matching engine status.
    pub flags: u8,
    /// The message sequence number assigned at the venue.
    pub sequence: u64,
    /// The UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// The UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl OrderBookDepth10 {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        bids: [BookOrder; DEPTH_10_LEN],
        asks: [BookOrder; DEPTH_10_LEN],
        flags: u8,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            bids,
            asks,
            flags,
            sequence,
            ts_event,
            ts_init,
        }
    }
}

// TODO: Exact format for Debug and Display TBD
impl Display for OrderBookDepth10 {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},flags={},sequence={},ts_event={},ts_init={}",
            self.instrument_id, self.flags, self.sequence, self.ts_event, self.ts_init
        )
    }
}

impl Serializable for OrderBookDepth10 {}

////////////////////////////////////////////////////////////////////////////////
// Stubs
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod stubs {
    use rstest::fixture;

    use super::*;
    use crate::{
        data::order::BookOrder,
        enums::OrderSide,
        identifiers::instrument_id::InstrumentId,
        types::{price::Price, quantity::Quantity},
    };

    #[fixture]
    pub fn stub_depth10() -> OrderBookDepth10 {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let flags = 0;
        let sequence = 0;
        let ts_event = 1;
        let ts_init = 2;

        let mut bids: [BookOrder; DEPTH_10_LEN] = [BookOrder::default(); DEPTH_10_LEN];
        let mut asks: [BookOrder; DEPTH_10_LEN] = [BookOrder::default(); DEPTH_10_LEN];

        // Create bids
        let mut price = 99.00;
        let mut quantity = 100.0;
        let mut order_id = 1;

        for i in 0..10 {
            let order = BookOrder::new(
                OrderSide::Sell,
                Price::new(price, 2).unwrap(),
                Quantity::new(quantity, 0).unwrap(),
                order_id,
            );

            bids[i] = order;

            price -= 1.0;
            quantity += 100.0;
            order_id += 1;
        }

        // Create asks
        let mut price = 100.00;
        let mut quantity = 100.0;
        let mut order_id = 11;

        for i in 0..10 {
            let order = BookOrder::new(
                OrderSide::Buy,
                Price::new(price, 2).unwrap(),
                Quantity::new(quantity, 0).unwrap(),
                order_id,
            );

            asks[i] = order;

            price += 1.0;
            quantity += 100.0;
            order_id += 1;
        }

        OrderBookDepth10::new(
            instrument_id,
            bids,
            asks,
            flags,
            sequence,
            ts_event,
            ts_init,
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{stubs::*, *};

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
            format!("{}", depth),
            "AAPL.XNAS,flags=0,sequence=0,ts_event=1,ts_init=2".to_string()
        );
    }
}
