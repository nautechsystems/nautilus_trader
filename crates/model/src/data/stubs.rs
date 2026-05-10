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

//! Type stubs to facilitate testing.

use std::sync::Arc;

use nautilus_core::{Params, UnixNanos};
use rstest::fixture;
use serde::{Deserialize, Serialize};

use super::{
    Bar, BarSpecification, BarType, CustomData, CustomDataTrait, DEPTH10_LEN, DataType, HasTsInit,
    InstrumentStatus, OrderBookDelta, OrderBookDeltas, OrderBookDepth10, QuoteTick, TradeTick,
    close::InstrumentClose, register_custom_data_json,
};
use crate::{
    data::order::BookOrder,
    enums::{
        AggregationSource, AggressorSide, BarAggregation, BookAction, InstrumentCloseType,
        MarketStatusAction, OrderSide, PriceType,
    },
    identifiers::{InstrumentId, Symbol, TradeId, Venue},
    types::{Price, Quantity},
};

impl Default for QuoteTick {
    /// Creates a new default [`QuoteTick`] instance for testing.
    fn default() -> Self {
        Self {
            instrument_id: InstrumentId::from("AUDUSD.SIM"),
            bid_price: Price::from("1.00000"),
            ask_price: Price::from("1.00000"),
            bid_size: Quantity::from(100_000),
            ask_size: Quantity::from(100_000),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }
}

impl Default for TradeTick {
    /// Creates a new default [`TradeTick`] instance for testing.
    fn default() -> Self {
        Self {
            instrument_id: InstrumentId::from("AUDUSD.SIM"),
            price: Price::from("1.00000"),
            size: Quantity::from(100_000),
            aggressor_side: AggressorSide::Buyer,
            trade_id: TradeId::new("123456789"),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }
}

impl Default for Bar {
    /// Creates a new default [`Bar`] instance for testing.
    fn default() -> Self {
        Self {
            bar_type: BarType::from("AUDUSD.SIM-1-MINUTE-LAST-INTERNAL"),
            open: Price::from("1.00010"),
            high: Price::from("1.00020"),
            low: Price::from("1.00000"),
            close: Price::from("1.00010"),
            volume: Quantity::from(100_000),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }
}

#[fixture]
pub fn stub_delta() -> OrderBookDelta {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let action = BookAction::Add;
    let price = Price::from("100.00");
    let size = Quantity::from("10");
    let side = OrderSide::Buy;
    let order_id = 123_456;
    let flags = 0;
    let sequence = 1;
    let ts_event = 1;
    let ts_init = 2;

    let order = BookOrder::new(side, price, size, order_id);
    OrderBookDelta::new(
        instrument_id,
        action,
        order,
        flags,
        sequence,
        ts_event.into(),
        ts_init.into(),
    )
}

#[fixture]
pub fn stub_deltas() -> OrderBookDeltas {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let flags = 32; // Snapshot flag
    let sequence = 0;
    let ts_event = 1;
    let ts_init = 2;

    let delta0 = OrderBookDelta::clear(instrument_id, sequence, ts_event.into(), ts_init.into());
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

    let deltas = vec![delta0, delta1, delta2, delta3, delta4, delta5, delta6];

    OrderBookDeltas::new(instrument_id, deltas)
}

#[fixture]
pub fn stub_depth10() -> OrderBookDepth10 {
    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let flags = 0;
    let sequence = 0;
    let ts_event = 1;
    let ts_init = 2;

    let mut bids: [BookOrder; DEPTH10_LEN] = [BookOrder::default(); DEPTH10_LEN];
    let mut asks: [BookOrder; DEPTH10_LEN] = [BookOrder::default(); DEPTH10_LEN];

    // Create bids
    let mut price = 99.00;
    let mut quantity = 100.0;

    for (i, bid) in bids.iter_mut().enumerate() {
        *bid = BookOrder::new(
            OrderSide::Buy,
            Price::new(price, 2),
            Quantity::new(quantity, 0),
            (i + 1) as u64,
        );

        price -= 1.0;
        quantity += 100.0;
    }

    // Create asks
    let mut price = 100.00;
    let mut quantity = 100.0;

    for (i, ask) in asks.iter_mut().enumerate() {
        *ask = BookOrder::new(
            OrderSide::Sell,
            Price::new(price, 2),
            Quantity::new(quantity, 0),
            (i + 11) as u64,
        );

        price += 1.0;
        quantity += 100.0;
    }

    let bid_counts: [u32; DEPTH10_LEN] = [1; DEPTH10_LEN];
    let ask_counts: [u32; DEPTH10_LEN] = [1; DEPTH10_LEN];

    OrderBookDepth10::new(
        instrument_id,
        bids,
        asks,
        bid_counts,
        ask_counts,
        flags,
        sequence,
        ts_event.into(),
        ts_init.into(),
    )
}

#[fixture]
pub fn stub_book_order() -> BookOrder {
    let price = Price::from("100.00");
    let size = Quantity::from("10");
    let side = OrderSide::Buy;
    let order_id = 123_456;

    BookOrder::new(side, price, size, order_id)
}

#[fixture]
pub fn quote_ethusdt_binance() -> QuoteTick {
    QuoteTick {
        instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        bid_price: Price::from("10000.0000"),
        ask_price: Price::from("10001.0000"),
        bid_size: Quantity::from("1.00000000"),
        ask_size: Quantity::from("1.00000000"),
        ts_event: UnixNanos::default(),
        ts_init: UnixNanos::from(1),
    }
}

#[fixture]
pub fn quote_audusd() -> QuoteTick {
    QuoteTick {
        instrument_id: InstrumentId::from("AUD/USD.SIM"),
        bid_price: Price::from("100.0000"),
        ask_price: Price::from("101.0000"),
        bid_size: Quantity::from("1.00000000"),
        ask_size: Quantity::from("1.00000000"),
        ts_event: UnixNanos::default(),
        ts_init: UnixNanos::from(1),
    }
}

#[fixture]
pub fn stub_trade_ethusdt_buyer() -> TradeTick {
    TradeTick {
        instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        price: Price::from("10000.0000"),
        size: Quantity::from("1.00000000"),
        aggressor_side: AggressorSide::Buyer,
        trade_id: TradeId::new("123456789"),
        ts_event: UnixNanos::default(),
        ts_init: UnixNanos::from(1),
    }
}

#[fixture]
pub fn stub_bar() -> Bar {
    let instrument_id = InstrumentId {
        symbol: Symbol::new("AUD/USD"),
        venue: Venue::new("SIM"),
    };
    let bar_spec = BarSpecification::new(1, BarAggregation::Minute, PriceType::Bid);
    let bar_type = BarType::Standard {
        instrument_id,
        spec: bar_spec,
        aggregation_source: AggregationSource::External,
    };
    Bar {
        bar_type,
        open: Price::from("1.00002"),
        high: Price::from("1.00004"),
        low: Price::from("1.00001"),
        close: Price::from("1.00003"),
        volume: Quantity::from("100000"),
        ts_event: UnixNanos::default(),
        ts_init: UnixNanos::from(1),
    }
}

#[fixture]
pub fn stub_instrument_status() -> InstrumentStatus {
    let instrument_id = InstrumentId::from("MSFT.XNAS");
    InstrumentStatus::new(
        instrument_id,
        MarketStatusAction::Trading,
        UnixNanos::from(1),
        UnixNanos::from(2),
        None,
        None,
        None,
        None,
        None,
    )
}

#[fixture]
pub fn stub_instrument_close() -> InstrumentClose {
    let instrument_id = InstrumentId::from("MSFT.XNAS");
    InstrumentClose::new(
        instrument_id,
        Price::from("100.50"),
        InstrumentCloseType::EndOfSession,
        UnixNanos::from(1),
        UnixNanos::from(2),
    )
}

#[derive(Debug)]
pub struct OrderBookDeltaTestBuilder {
    instrument_id: InstrumentId,
    action: Option<BookAction>,
    book_order: Option<BookOrder>,
    flags: Option<u8>,
    sequence: Option<u64>,
    ts_event: Option<UnixNanos>,
}

impl OrderBookDeltaTestBuilder {
    #[must_use]
    pub fn new(instrument_id: InstrumentId) -> Self {
        Self {
            instrument_id,
            action: None,
            book_order: None,
            flags: None,
            sequence: None,
            ts_event: None,
        }
    }

    pub fn book_action(&mut self, action: BookAction) -> &mut Self {
        self.action = Some(action);
        self
    }

    fn get_book_action(&self) -> BookAction {
        self.action.unwrap_or(BookAction::Add)
    }

    pub fn book_order(&mut self, book_order: BookOrder) -> &mut Self {
        self.book_order = Some(book_order);
        self
    }

    fn get_book_order(&self) -> BookOrder {
        self.book_order.unwrap_or(BookOrder::new(
            OrderSide::Sell,
            Price::from("1500.00"),
            Quantity::from("1"),
            1,
        ))
    }

    pub fn flags(&mut self, flags: u8) -> &mut Self {
        self.flags = Some(flags);
        self
    }

    fn get_flags(&self) -> u8 {
        self.flags.unwrap_or(0)
    }

    pub fn sequence(&mut self, sequence: u64) -> &mut Self {
        self.sequence = Some(sequence);
        self
    }

    fn get_sequence(&self) -> u64 {
        self.sequence.unwrap_or(1)
    }

    pub fn ts_event(&mut self, ts_event: UnixNanos) -> &mut Self {
        self.ts_event = Some(ts_event);
        self
    }

    #[must_use]
    pub fn build(&self) -> OrderBookDelta {
        OrderBookDelta::new(
            self.instrument_id,
            self.get_book_action(),
            self.get_book_order(),
            self.get_flags(),
            self.get_sequence(),
            self.ts_event.unwrap_or(UnixNanos::from(1)),
            UnixNanos::from(2),
        )
    }
}

/// Stub custom data type for integration tests (e.g. Redis cache).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StubCustomData {
    pub ts_init: UnixNanos,
    pub value: i64,
}

impl HasTsInit for StubCustomData {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for StubCustomData {
    fn type_name(&self) -> &'static str {
        "StubCustomData"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn ts_event(&self) -> UnixNanos {
        self.ts_init
    }
    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }
    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }
    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        if let Some(o) = other.as_any().downcast_ref::<Self>() {
            self == o
        } else {
            false
        }
    }

    fn type_name_static() -> &'static str {
        "StubCustomData"
    }
    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        let parsed: Self = serde_json::from_value(value)?;
        Ok(Arc::new(parsed))
    }
}

/// Registers `StubCustomData` for JSON roundtrip; call once before tests that persist custom data.
pub fn ensure_stub_custom_data_registered() {
    let _ = register_custom_data_json::<StubCustomData>();
}

/// Builds a `CustomData` stub for tests (e.g. Redis add/load).
#[must_use]
pub fn stub_custom_data(
    ts_init: u64,
    value: i64,
    metadata: Option<Params>,
    identifier: Option<String>,
) -> CustomData {
    ensure_stub_custom_data_registered();
    let inner = StubCustomData {
        ts_init: UnixNanos::from(ts_init),
        value,
    };
    let data_type = DataType::new("StubCustomData", metadata, identifier);
    CustomData::new(Arc::new(inner), data_type)
}
