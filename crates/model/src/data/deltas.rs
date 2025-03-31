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

//! An `OrderBookDeltas` container type to carry a bulk of `OrderBookDelta` records.

use std::{
    fmt::Display,
    hash::{Hash, Hasher},
    ops::{Deref, DerefMut},
};

use nautilus_core::{
    UnixNanos,
    correctness::{FAILED, check_predicate_true},
};
use serde::{Deserialize, Serialize};

use super::{GetTsInit, OrderBookDelta};
use crate::identifiers::InstrumentId;

/// Represents a grouped batch of `OrderBookDelta` updates for an `OrderBook`.
///
/// This type cannot be `repr(C)` due to the `deltas` vec.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderBookDeltas {
    /// The instrument ID for the book.
    pub instrument_id: InstrumentId,
    /// The order book deltas.
    pub deltas: Vec<OrderBookDelta>,
    /// The record flags bit field, indicating event end and data information.
    pub flags: u8,
    /// The message sequence number assigned at the venue.
    pub sequence: u64,
    /// UNIX timestamp (nanoseconds) when the book event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the struct was initialized.
    pub ts_init: UnixNanos,
}

impl OrderBookDeltas {
    /// Creates a new [`OrderBookDeltas`] instance.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(instrument_id: InstrumentId, deltas: Vec<OrderBookDelta>) -> Self {
        Self::new_checked(instrument_id, deltas).expect(FAILED)
    }

    /// Creates a new [`OrderBookDeltas`] instance with correctness checking.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    #[allow(clippy::too_many_arguments)]
    pub fn new_checked(
        instrument_id: InstrumentId,
        deltas: Vec<OrderBookDelta>,
    ) -> anyhow::Result<Self> {
        check_predicate_true(!deltas.is_empty(), "`deltas` cannot be empty")?;
        // SAFETY: We asserted `deltas` is not empty
        let last = deltas.last().unwrap();
        let flags = last.flags;
        let sequence = last.sequence;
        let ts_event = last.ts_event;
        let ts_init = last.ts_init;
        Ok(Self {
            instrument_id,
            deltas,
            flags,
            sequence,
            ts_event,
            ts_init,
        })
    }
}

impl PartialEq<Self> for OrderBookDeltas {
    fn eq(&self, other: &Self) -> bool {
        self.instrument_id == other.instrument_id && self.sequence == other.sequence
    }
}

impl Eq for OrderBookDeltas {}

impl Hash for OrderBookDeltas {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.instrument_id.hash(state);
        self.sequence.hash(state);
    }
}

// TODO: Implement
// impl Serializable for OrderBookDeltas {}

// TODO: Exact format for Debug and Display TBD
impl Display for OrderBookDeltas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

impl GetTsInit for OrderBookDeltas {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

/// C compatible Foreign Function Interface (FFI) for an underlying [`OrderBookDeltas`].
///
/// This struct wraps `OrderBookDeltas` in a way that makes it compatible with C function
/// calls, enabling interaction with `OrderBookDeltas` in a C environment.
///
/// It implements the `Deref` trait, allowing instances of `OrderBookDeltas_API` to be
/// dereferenced to `OrderBookDeltas`, providing access to `OrderBookDeltas`'s methods without
/// having to manually access the underlying `OrderBookDeltas` instance.
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub struct OrderBookDeltas_API(Box<OrderBookDeltas>);

// TODO: This wrapper will go along with Cython
impl OrderBookDeltas_API {
    #[must_use]
    pub fn new(deltas: OrderBookDeltas) -> Self {
        Self(Box::new(deltas))
    }

    /// Consumes the wrapper and returns the inner `OrderBookDeltas`.
    #[must_use]
    pub fn into_inner(self) -> OrderBookDeltas {
        *self.0
    }
}

impl Deref for OrderBookDeltas_API {
    type Target = OrderBookDeltas;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for OrderBookDeltas_API {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        data::{order::BookOrder, stubs::stub_deltas},
        enums::{BookAction, OrderSide},
        types::{Price, Quantity},
    };

    #[rstest]
    fn test_new() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let flags = 32; // Snapshot flag
        let sequence = 0;
        let ts_event = 1;
        let ts_init = 2;

        let delta0 =
            OrderBookDelta::clear(instrument_id, sequence, ts_event.into(), ts_init.into());
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
            ts_event.into(),
            ts_init.into(),
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
            ts_event.into(),
            ts_init.into(),
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
            ts_event.into(),
            ts_init.into(),
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
            ts_event.into(),
            ts_init.into(),
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
            ts_event.into(),
            ts_init.into(),
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
            ts_event.into(),
            ts_init.into(),
        );

        let deltas = OrderBookDeltas::new(
            instrument_id,
            vec![delta0, delta1, delta2, delta3, delta4, delta5, delta6],
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
            format!("{deltas}"),
            "AAPL.XNAS,len=7,flags=32,sequence=0,ts_event=1,ts_init=2".to_string()
        );
    }
}
