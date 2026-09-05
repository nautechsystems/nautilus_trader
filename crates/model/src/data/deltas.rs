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

//! An `OrderBookDeltas` container type to carry a bulk of `OrderBookDelta` records.

use std::{
    fmt::Display,
    hash::{Hash, Hasher},
};

use nautilus_core::{
    UnixNanos,
    correctness::{FAILED, check_predicate_true},
    serialization::Serializable,
};
use serde::{Deserialize, Serialize};

use super::{HasTsInit, OrderBookDelta};
use crate::{enums::RecordFlag, identifiers::InstrumentId};

/// Represents a grouped batch of `OrderBookDelta` updates for an `OrderBook`.
///
/// This type cannot be `repr(C)` due to the `deltas` vec.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
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
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: UnixNanos,
}

impl OrderBookDeltas {
    /// Creates a new [`OrderBookDeltas`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `deltas` is empty or contains an instrument ID that does not match
    /// `instrument_id`.
    #[must_use]
    pub fn new(instrument_id: InstrumentId, deltas: Vec<OrderBookDelta>) -> Self {
        Self::new_checked(instrument_id, deltas).expect(FAILED)
    }

    /// Creates a new [`OrderBookDeltas`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if `deltas` is empty or contains an instrument ID that does not match
    /// `instrument_id`.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    #[expect(
        clippy::missing_panics_doc,
        reason = "the unwrapped last element is guarded by the non-empty check"
    )]
    pub fn new_checked(
        instrument_id: InstrumentId,
        deltas: Vec<OrderBookDelta>,
    ) -> anyhow::Result<Self> {
        check_predicate_true(!deltas.is_empty(), "`deltas` cannot be empty")?;

        let mismatch = deltas.iter().enumerate().find(|(_, delta)| {
            instrument_id != delta.instrument_id
                && (instrument_id.symbol.as_str() != delta.instrument_id.symbol.as_str()
                    || instrument_id.venue.as_str() != delta.instrument_id.venue.as_str())
        });

        if let Some((index, delta)) = mismatch {
            check_predicate_true(
                false,
                &format!(
                    "`deltas` instrument IDs must match `instrument_id` {instrument_id}, but \
                     delta at index {index} of {} has {}",
                    deltas.len(),
                    delta.instrument_id,
                ),
            )?;
        }
        let last = deltas.last().expect("deltas not empty");
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

    /// Returns whether the batch is a snapshot.
    #[cfg_attr(not(any(feature = "ffi", feature = "python")), allow(dead_code))]
    #[must_use]
    pub(crate) fn is_snapshot(&self) -> bool {
        RecordFlag::F_SNAPSHOT.matches(self.flags)
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

impl Serializable for OrderBookDeltas {}

impl HasTsInit for OrderBookDeltas {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };

    use nautilus_core::serialization::{
        Serializable,
        msgpack::{FromMsgPack, ToMsgPack},
    };
    use rstest::rstest;
    use serde_json;

    use super::*;
    use crate::{
        data::{order::BookOrder, stubs::stub_deltas},
        enums::{BookAction, OrderSide, RecordFlag},
        types::{Price, Quantity},
    };

    fn create_test_delta() -> OrderBookDelta {
        let instrument_id = InstrumentId::from("EURUSD.SIM");
        OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("1.0500"),
                Quantity::from("100000"),
                1,
            ),
            0,
            123,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        )
    }

    fn create_test_deltas() -> OrderBookDeltas {
        let instrument_id = InstrumentId::from("EURUSD.SIM");
        let flags = 32;
        let sequence = 123;
        let ts_event = UnixNanos::from(1_000_000_000);
        let ts_init = UnixNanos::from(2_000_000_000);

        let delta1 = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("1.0520"),
                Quantity::from("50000"),
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
                OrderSide::Buy,
                Price::from("1.0500"),
                Quantity::from("75000"),
                2,
            ),
            flags,
            sequence,
            ts_event,
            ts_init,
        );

        OrderBookDeltas::new(instrument_id, vec![delta1, delta2])
    }

    fn create_test_deltas_multiple() -> OrderBookDeltas {
        let instrument_id = InstrumentId::from("GBPUSD.SIM");
        let flags = 16;
        let sequence = 456;
        let ts_event = UnixNanos::from(3_000_000_000);
        let ts_init = UnixNanos::from(4_000_000_000);

        let deltas = vec![
            OrderBookDelta::clear(instrument_id, sequence, ts_event, ts_init),
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from("1.2550"),
                    Quantity::from("100000"),
                    1,
                ),
                flags,
                sequence,
                ts_event,
                ts_init,
            ),
            OrderBookDelta::new(
                instrument_id,
                BookAction::Update,
                BookOrder::new(
                    OrderSide::Buy,
                    Price::from("1.2530"),
                    Quantity::from("200000"),
                    2,
                ),
                flags,
                sequence,
                ts_event,
                ts_init,
            ),
            OrderBookDelta::new(
                instrument_id,
                BookAction::Delete,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from("1.2560"),
                    Quantity::from("0"),
                    3,
                ),
                flags,
                sequence,
                ts_event,
                ts_init,
            ),
        ];

        OrderBookDeltas::new(instrument_id, deltas)
    }

    // Compared field by field, as `PartialEq` covers only `instrument_id` and `sequence` here,
    // and only `order_id` on the nested `BookOrder`.
    fn assert_book_order_fields(expected: &BookOrder, actual: &BookOrder) {
        assert_eq!(expected.side, actual.side);
        assert_eq!(expected.price, actual.price);
        assert_eq!(expected.size, actual.size);
        assert_eq!(expected.order_id, actual.order_id);
    }

    fn assert_order_book_delta_fields(expected: &OrderBookDelta, actual: &OrderBookDelta) {
        assert_eq!(expected.instrument_id, actual.instrument_id);
        assert_eq!(expected.action, actual.action);
        assert_book_order_fields(&expected.order, &actual.order);
        assert_eq!(expected.flags, actual.flags);
        assert_eq!(expected.sequence, actual.sequence);
        assert_eq!(expected.ts_event, actual.ts_event);
        assert_eq!(expected.ts_init, actual.ts_init);
    }

    fn assert_order_book_deltas_fields(expected: &OrderBookDeltas, actual: &OrderBookDeltas) {
        assert_eq!(expected.instrument_id, actual.instrument_id);
        assert_eq!(expected.flags, actual.flags);
        assert_eq!(expected.sequence, actual.sequence);
        assert_eq!(expected.ts_event, actual.ts_event);
        assert_eq!(expected.ts_init, actual.ts_init);
        assert_eq!(expected.deltas.len(), actual.deltas.len());

        for (expected_delta, actual_delta) in expected.deltas.iter().zip(&actual.deltas) {
            assert_order_book_delta_fields(expected_delta, actual_delta);
        }
    }

    #[rstest]
    fn test_order_book_deltas_new() {
        let deltas = create_test_deltas();

        assert_eq!(deltas.instrument_id, InstrumentId::from("EURUSD.SIM"));
        assert_eq!(deltas.deltas.len(), 2);
        assert_eq!(deltas.flags, 32);
        assert_eq!(deltas.sequence, 123);
        assert_eq!(deltas.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(deltas.ts_init, UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_order_book_deltas_new_checked_valid() {
        let instrument_id = InstrumentId::from("EURUSD.SIM");
        let delta = create_test_delta();

        let result = OrderBookDeltas::new_checked(instrument_id, vec![delta]);

        assert!(result.is_ok());
        let deltas = result.unwrap();
        assert_eq!(deltas.instrument_id, instrument_id);
        assert_eq!(deltas.deltas.len(), 1);
    }

    #[rstest]
    fn test_order_book_deltas_new_checked_accepts_homogeneous_deltas() {
        let instrument_id = InstrumentId::from("EURUSD.SIM");
        let delta1 = create_test_delta();
        let mut delta2 = create_test_delta();
        delta2.sequence = 124;

        let result = OrderBookDeltas::new_checked(instrument_id, vec![delta1, delta2]);

        assert!(result.is_ok());
        assert_eq!(result.unwrap().deltas.len(), 2);
    }

    #[rstest]
    #[case::first(0)]
    #[case::later(1)]
    fn test_order_book_deltas_new_checked_rejects_mismatched_instrument(
        #[case] mismatch_index: usize,
    ) {
        let instrument_id = InstrumentId::from("EURUSD.SIM");
        let mut deltas = vec![create_test_delta(), create_test_delta()];
        deltas[mismatch_index].instrument_id = InstrumentId::from("GBPUSD.SIM");

        let result = OrderBookDeltas::new_checked(instrument_id, deltas);

        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "`deltas` instrument IDs must match `instrument_id` EURUSD.SIM, but delta at \
                 index {mismatch_index} of 2 has GBPUSD.SIM"
            )
        );
    }

    #[rstest]
    fn test_order_book_deltas_new_checked_empty_deltas() {
        let instrument_id = InstrumentId::from("EURUSD.SIM");

        let result = OrderBookDeltas::new_checked(instrument_id, vec![]);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("`deltas` cannot be empty")
        );
    }

    #[rstest]
    #[should_panic(expected = "Condition failed")]
    fn test_order_book_deltas_new_empty_deltas_panics() {
        let instrument_id = InstrumentId::from("EURUSD.SIM");
        let _ = OrderBookDeltas::new(instrument_id, vec![]);
    }

    #[rstest]
    fn test_order_book_deltas_uses_last_delta_properties() {
        let instrument_id = InstrumentId::from("EURUSD.SIM");

        let delta1 = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("1.0500"),
                Quantity::from("100000"),
                1,
            ),
            16,                             // Different flags
            100,                            // Different sequence
            UnixNanos::from(500_000_000),   // Different ts_event
            UnixNanos::from(1_000_000_000), // Different ts_init
        );

        let delta2 = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("1.0520"),
                Quantity::from("50000"),
                2,
            ),
            32,                             // Final flags
            200,                            // Final sequence
            UnixNanos::from(1_500_000_000), // Final ts_event
            UnixNanos::from(2_000_000_000), // Final ts_init
        );

        let deltas = OrderBookDeltas::new(instrument_id, vec![delta1, delta2]);

        // Should use properties from the last delta
        assert_eq!(deltas.flags, 32);
        assert_eq!(deltas.sequence, 200);
        assert_eq!(deltas.ts_event, UnixNanos::from(1_500_000_000));
        assert_eq!(deltas.ts_init, UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    #[case::snapshot(
        vec![(BookAction::Add, RecordFlag::F_SNAPSHOT as u8)],
        true
    )]
    #[case::combined_flags(
        vec![(
            BookAction::Add,
            RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8,
        )],
        true
    )]
    #[case::not_snapshot(vec![(BookAction::Add, RecordFlag::F_MBP as u8)], false)]
    #[case::clear_without_snapshot(
        vec![(BookAction::Clear, 0), (BookAction::Add, 0)],
        false
    )]
    fn test_order_book_deltas_is_snapshot(
        #[case] actions_and_flags: Vec<(BookAction, u8)>,
        #[case] expected: bool,
    ) {
        let instrument_id = InstrumentId::from("EURUSD.SIM");
        let deltas = actions_and_flags
            .into_iter()
            .map(|(action, flags)| {
                let mut delta = create_test_delta();
                delta.action = action;
                delta.flags = flags;
                delta
            })
            .collect();
        let deltas = OrderBookDeltas::new(instrument_id, deltas);

        assert_eq!(deltas.is_snapshot(), expected);
    }

    #[rstest]
    fn test_order_book_deltas_hash_different_objects() {
        let deltas1 = create_test_deltas();
        let deltas2 = create_test_deltas_multiple();

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        deltas1.hash(&mut hasher1);
        deltas2.hash(&mut hasher2);

        assert_ne!(hasher1.finish(), hasher2.finish()); // Different objects should have different hashes
    }

    #[rstest]
    fn test_order_book_deltas_hash_uses_instrument_id_and_sequence() {
        let instrument_id = InstrumentId::from("EURUSD.SIM");
        let sequence = 123u64;

        // Create separate hasher to verify what's being hashed
        let mut expected_hasher = DefaultHasher::new();
        instrument_id.hash(&mut expected_hasher);
        sequence.hash(&mut expected_hasher);
        let expected_hash = expected_hasher.finish();

        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("1.0500"),
                Quantity::from("100000"),
                1,
            ),
            0,
            sequence,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        );

        let deltas = OrderBookDeltas::new(instrument_id, vec![delta]);

        let mut deltas_hasher = DefaultHasher::new();
        deltas.hash(&mut deltas_hasher);

        assert_eq!(deltas_hasher.finish(), expected_hash);
    }

    #[rstest]
    fn test_order_book_deltas_display() {
        let deltas = create_test_deltas();
        let display_str = format!("{deltas}");

        assert!(display_str.contains("EURUSD.SIM"));
        assert!(display_str.contains("len=2"));
        assert!(display_str.contains("flags=32"));
        assert!(display_str.contains("sequence=123"));
        assert!(display_str.contains("ts_event=1000000000"));
        assert!(display_str.contains("ts_init=2000000000"));
    }

    #[rstest]
    fn test_order_book_deltas_display_format() {
        let deltas = create_test_deltas();
        let expected =
            "EURUSD.SIM,len=2,flags=32,sequence=123,ts_event=1000000000,ts_init=2000000000";

        assert_eq!(format!("{deltas}"), expected);
    }

    #[rstest]
    fn test_order_book_deltas_has_ts_init() {
        let deltas = create_test_deltas();

        assert_eq!(deltas.ts_init(), UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_order_book_deltas_clone() {
        let deltas1 = create_test_deltas();
        let deltas2 = deltas1.clone();

        assert_eq!(deltas1.instrument_id, deltas2.instrument_id);
        assert_eq!(deltas1.deltas.len(), deltas2.deltas.len());
        assert_eq!(deltas1.flags, deltas2.flags);
        assert_eq!(deltas1.sequence, deltas2.sequence);
        assert_eq!(deltas1.ts_event, deltas2.ts_event);
        assert_eq!(deltas1.ts_init, deltas2.ts_init);
        assert_eq!(deltas1, deltas2);
    }

    #[rstest]
    fn test_order_book_deltas_debug() {
        let deltas = create_test_deltas();
        let debug_str = format!("{deltas:?}");

        assert!(debug_str.contains("OrderBookDeltas"));
        assert!(debug_str.contains("EURUSD.SIM"));
        assert!(debug_str.contains("flags: 32"));
        assert!(debug_str.contains("sequence: 123"));
    }

    #[rstest]
    fn test_order_book_deltas_serialization() {
        let deltas = create_test_deltas();

        // Test JSON serialization
        let json = serde_json::to_string(&deltas).unwrap();
        let deserialized: OrderBookDeltas = serde_json::from_str(&json).unwrap();

        assert_eq!(deltas.instrument_id, deserialized.instrument_id);
        assert_eq!(deltas.deltas.len(), deserialized.deltas.len());
        assert_eq!(deltas.flags, deserialized.flags);
        assert_eq!(deltas.sequence, deserialized.sequence);
        assert_eq!(deltas.ts_event, deserialized.ts_event);
        assert_eq!(deltas.ts_init, deserialized.ts_init);
    }

    #[rstest]
    fn test_json_serialization(stub_deltas: OrderBookDeltas) {
        let deltas = stub_deltas;
        let serialized = deltas.to_json_bytes().unwrap();
        let deserialized = OrderBookDeltas::from_json_bytes(serialized.as_ref()).unwrap();

        assert_order_book_deltas_fields(&deltas, &deserialized);
    }

    #[rstest]
    fn test_msgpack_serialization() {
        let deltas = create_test_deltas_multiple();
        let serialized = deltas.to_msgpack_bytes().unwrap();
        let deserialized = OrderBookDeltas::from_msgpack_bytes(serialized.as_ref()).unwrap();

        assert_order_book_deltas_fields(&deltas, &deserialized);
    }

    #[rstest]
    fn test_order_book_deltas_single_delta() {
        let delta = create_test_delta();
        let instrument_id = delta.instrument_id;

        let deltas = OrderBookDeltas::new(instrument_id, vec![delta]);

        assert_eq!(deltas.instrument_id, instrument_id);
        assert_eq!(deltas.deltas.len(), 1);
        assert_eq!(deltas.flags, delta.flags);
        assert_eq!(deltas.sequence, delta.sequence);
        assert_eq!(deltas.ts_event, delta.ts_event);
        assert_eq!(deltas.ts_init, delta.ts_init);
    }

    #[rstest]
    fn test_order_book_deltas_large_number_of_deltas() {
        let instrument_id = InstrumentId::from("ETHUSD.CRYPTO");
        let mut delta_vec = Vec::new();

        // Create 100 deltas
        for i in 0..100 {
            let delta = OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Buy,
                    Price::from(&format!("1000.{i:02}")),
                    Quantity::from("1000"),
                    i as u64,
                ),
                0,
                i as u64,
                UnixNanos::from(1_000_000_000 + i as u64),
                UnixNanos::from(2_000_000_000 + i as u64),
            );
            delta_vec.push(delta);
        }

        let deltas = OrderBookDeltas::new(instrument_id, delta_vec);

        assert_eq!(deltas.deltas.len(), 100);
        assert_eq!(deltas.sequence, 99); // Last delta's sequence
        assert_eq!(deltas.ts_event, UnixNanos::from(1_000_000_000 + 99));
        assert_eq!(deltas.ts_init, UnixNanos::from(2_000_000_000 + 99));
    }

    #[rstest]
    fn test_order_book_deltas_different_action_types() {
        let deltas = create_test_deltas_multiple();

        assert_eq!(deltas.deltas.len(), 4);

        // Verify different action types are preserved
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[1].action, BookAction::Add);
        assert_eq!(deltas.deltas[2].action, BookAction::Update);
        assert_eq!(deltas.deltas[3].action, BookAction::Delete);
    }

    #[rstest]
    fn test_order_book_deltas_with_stub(stub_deltas: OrderBookDeltas) {
        let deltas = stub_deltas;

        assert_eq!(deltas.instrument_id, InstrumentId::from("AAPL.XNAS"));
        assert_eq!(deltas.deltas.len(), 7);
        assert_eq!(deltas.flags, 32);
        assert_eq!(deltas.sequence, 0);
        assert_eq!(deltas.ts_event, UnixNanos::from(1));
        assert_eq!(deltas.ts_init, UnixNanos::from(2));
    }

    #[rstest]
    fn test_display_with_stub(stub_deltas: OrderBookDeltas) {
        let deltas = stub_deltas;
        assert_eq!(
            format!("{deltas}"),
            "AAPL.XNAS,len=7,flags=32,sequence=0,ts_event=1,ts_init=2".to_string()
        );
    }

    #[rstest]
    fn test_order_book_deltas_zero_sequence() {
        let instrument_id = InstrumentId::from("ZERO.TEST");
        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("100.0"),
                Quantity::from("1000"),
                1,
            ),
            0,
            0,                  // Zero sequence
            UnixNanos::from(0), // Zero timestamp
            UnixNanos::from(0),
        );

        let deltas = OrderBookDeltas::new(instrument_id, vec![delta]);

        assert_eq!(deltas.sequence, 0);
        assert_eq!(deltas.ts_event, UnixNanos::from(0));
        assert_eq!(deltas.ts_init, UnixNanos::from(0));
    }

    #[rstest]
    fn test_order_book_deltas_max_values() {
        let instrument_id = InstrumentId::from("MAX.TEST");
        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("999999.99"),
                Quantity::from("999999999"),
                u64::MAX,
            ),
            u8::MAX,
            u64::MAX,
            UnixNanos::from(u64::MAX),
            UnixNanos::from(u64::MAX),
        );

        let deltas = OrderBookDeltas::new(instrument_id, vec![delta]);

        assert_eq!(deltas.flags, u8::MAX);
        assert_eq!(deltas.sequence, u64::MAX);
        assert_eq!(deltas.ts_event, UnixNanos::from(u64::MAX));
        assert_eq!(deltas.ts_init, UnixNanos::from(u64::MAX));
    }

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
}
