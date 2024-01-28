// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::time::UnixNanos;
use pyo3::prelude::*;

use super::delta::OrderBookDelta;
use crate::identifiers::instrument_id::InstrumentId;

/// Represents a grouped batch of `OrderBookDelta` updates for an `OrderBook`.
#[repr(C)]
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderBookDeltas {
    /// The instrument ID for the book.
    #[pyo3(get)]
    pub instrument_id: InstrumentId,
    /// The order book deltas.
    #[pyo3(get)]
    pub deltas: Vec<OrderBookDelta>,
    /// A combination of packet end with matching engine status.
    #[pyo3(get)]
    pub flags: u8,
    /// The message sequence number assigned at the venue.
    #[pyo3(get)]
    pub sequence: u64,
    /// The UNIX timestamp (nanoseconds) when the data event occurred.
    #[pyo3(get)]
    pub ts_event: UnixNanos,
    /// The UNIX timestamp (nanoseconds) when the data object was initialized.
    #[pyo3(get)]
    pub ts_init: UnixNanos,
}

impl OrderBookDeltas {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        deltas: Vec<OrderBookDelta>,
        flags: u8,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            deltas,
            flags,
            sequence,
            ts_event,
            ts_init,
        }
    }
}

// TODO: Potentially implement later
// impl Serializable for OrderBookDeltas {}

// TODO: Exact format for Debug and Display TBD
impl Display for OrderBookDeltas {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},len={},flags={},sequence={},ts_event={},ts_init={}",
            self.instrument_id,
            self.deltas.len(),
            self.flags,
            self.sequence,
            self.ts_event,
            self.ts_init
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// Stubs
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "stubs")]
pub mod stubs {
    use rstest::fixture;

    use super::*;
    use crate::{
        data::{delta::OrderBookDelta, order::BookOrder},
        enums::{BookAction, OrderSide},
        identifiers::instrument_id::InstrumentId,
        types::{price::Price, quantity::Quantity},
    };

    #[fixture]
    pub fn stub_deltas() -> OrderBookDeltas {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let flags = 32; // Snapshot flag
        let sequence = 0;
        let ts_event = 1;
        let ts_init = 2;

        let delta0 = OrderBookDelta::clear(instrument_id, sequence, ts_event, ts_init);
        let delta1 = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("102.00"),
                Quantity::from("300"),
                1,
            ),
            flags,
            sequence,
            ts_event,
            ts_init,
        );
        let delta2 = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("101.00"),
                Quantity::from("200"),
                2,
            ),
            flags,
            sequence,
            ts_event,
            ts_init,
        );
        let delta3 = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("100.00"),
                Quantity::from("100"),
                3,
            ),
            flags,
            sequence,
            ts_event,
            ts_init,
        );
        let delta4 = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("99.00"),
                Quantity::from("100"),
                4,
            ),
            flags,
            sequence,
            ts_event,
            ts_init,
        );
        let delta5 = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("98.00"),
                Quantity::from("200"),
                5,
            ),
            flags,
            sequence,
            ts_event,
            ts_init,
        );
        let delta6 = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("97.00"),
                Quantity::from("300"),
                6,
            ),
            flags,
            sequence,
            ts_event,
            ts_init,
        );

        let deltas = vec![delta0, delta1, delta2, delta3, delta4, delta5, delta6];

        OrderBookDeltas::new(instrument_id, deltas, flags, sequence, ts_event, ts_init)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{stubs::*, *};
    use crate::{
        data::order::BookOrder,
        enums::{BookAction, OrderSide},
        types::{price::Price, quantity::Quantity},
    };

    #[rstest]
    fn test_new() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let flags = 32; // Snapshot flag
        let sequence = 0;
        let ts_event = 1;
        let ts_init = 2;

        let delta0 = OrderBookDelta::clear(instrument_id, sequence, ts_event, ts_init);
        let delta1 = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("102.00"),
                Quantity::from("300"),
                1,
            ),
            flags,
            sequence,
            ts_event,
            ts_init,
        );
        let delta2 = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("101.00"),
                Quantity::from("200"),
                2,
            ),
            flags,
            sequence,
            ts_event,
            ts_init,
        );
        let delta3 = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("100.00"),
                Quantity::from("100"),
                3,
            ),
            flags,
            sequence,
            ts_event,
            ts_init,
        );
        let delta4 = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("99.00"),
                Quantity::from("100"),
                4,
            ),
            flags,
            sequence,
            ts_event,
            ts_init,
        );
        let delta5 = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("98.00"),
                Quantity::from("200"),
                5,
            ),
            flags,
            sequence,
            ts_event,
            ts_init,
        );
        let delta6 = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("97.00"),
                Quantity::from("300"),
                6,
            ),
            flags,
            sequence,
            ts_event,
            ts_init,
        );

        let deltas = OrderBookDeltas::new(
            instrument_id,
            vec![delta0, delta1, delta2, delta3, delta4, delta5, delta6],
            flags,
            sequence,
            ts_event,
            ts_init,
        );

        assert_eq!(deltas.instrument_id, instrument_id);
        assert_eq!(deltas.deltas.len(), 7);
        assert_eq!(deltas.flags, flags);
        assert_eq!(deltas.sequence, sequence);
        assert_eq!(deltas.ts_event, ts_event);
        assert_eq!(deltas.ts_init, ts_init);
    }

    // TODO: Exact format for Debug and Display TBD
    #[rstest]
    fn test_display(stub_deltas: OrderBookDeltas) {
        let deltas = stub_deltas;
        assert_eq!(
            format!("{}", deltas),
            "AAPL.XNAS,len=7,flags=32,sequence=0,ts_event=1,ts_init=2".to_string()
        );
    }
}
