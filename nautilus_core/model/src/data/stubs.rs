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

//! Type stubs to facilitate testing.

use nautilus_core::nanos::UnixNanos;
use rstest::fixture;

use super::{
    bar::{Bar, BarSpecification, BarType},
    deltas::OrderBookDeltas,
    depth::DEPTH10_LEN,
    quote::QuoteTick,
    trade::TradeTick,
    OrderBookDelta, OrderBookDepth10,
};
use crate::{
    data::order::BookOrder,
    enums::{AggregationSource, AggressorSide, BarAggregation, BookAction, OrderSide, PriceType},
    identifiers::{instrument_id::InstrumentId, symbol::Symbol, trade_id::TradeId, venue::Venue},
    types::{price::Price, quantity::Quantity},
};

impl Default for QuoteTick {
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
    fn default() -> Self {
        TradeTick {
            instrument_id: InstrumentId::from("AUDUSD.SIM"),
            price: Price::from("1.00000"),
            size: Quantity::from(100_000),
            aggressor_side: AggressorSide::Buyer,
            trade_id: TradeId::new("123456789").unwrap(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }
}

impl Default for Bar {
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
    let mut order_id = 1;

    #[allow(clippy::needless_range_loop)]
    for i in 0..DEPTH10_LEN {
        let order = BookOrder::new(
            OrderSide::Buy,
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

    #[allow(clippy::needless_range_loop)]
    for i in 0..DEPTH10_LEN {
        let order = BookOrder::new(
            OrderSide::Sell,
            Price::new(price, 2).unwrap(),
            Quantity::new(quantity, 0).unwrap(),
            order_id,
        );

        asks[i] = order;

        price += 1.0;
        quantity += 100.0;
        order_id += 1;
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
pub fn quote_tick_audusd_sim() -> QuoteTick {
    QuoteTick::default()
}

#[fixture]
pub fn quote_tick_ethusdt_binance() -> QuoteTick {
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
pub fn trade_tick_audusd_sim() -> TradeTick {
    TradeTick::default()
}

#[fixture]
pub fn stub_trade_tick_ethusdt_buyer() -> TradeTick {
    TradeTick {
        instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        price: Price::from("10000.0000"),
        size: Quantity::from("1.00000000"),
        aggressor_side: AggressorSide::Buyer,
        trade_id: TradeId::new("123456789").unwrap(),
        ts_event: UnixNanos::default(),
        ts_init: UnixNanos::from(1),
    }
}

#[fixture]
pub fn stub_bar() -> Bar {
    let instrument_id = InstrumentId {
        symbol: Symbol::new("AUDUSD").unwrap(),
        venue: Venue::new("SIM").unwrap(),
    };
    let bar_spec = BarSpecification {
        step: 1,
        aggregation: BarAggregation::Minute,
        price_type: PriceType::Bid,
    };
    let bar_type = BarType {
        instrument_id,
        spec: bar_spec,
        aggregation_source: AggregationSource::External,
    };
    Bar {
        bar_type,
        open: Price::from("1.00001"),
        high: Price::from("1.00004"),
        low: Price::from("1.00002"),
        close: Price::from("1.00003"),
        volume: Quantity::from("100000"),
        ts_event: UnixNanos::default(),
        ts_init: UnixNanos::from(1),
    }
}
