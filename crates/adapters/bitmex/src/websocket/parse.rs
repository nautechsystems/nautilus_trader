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

use std::num::NonZero;

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Data,
        bar::{Bar, BarSpecification, BarType},
        delta::OrderBookDelta,
        depth::{DEPTH10_LEN, OrderBookDepth10},
        order::BookOrder,
        quote::QuoteTick,
        trade::TradeTick,
    },
    enums::{AggregationSource, BarAggregation, OrderSide, PriceType, RecordFlag},
    identifiers::TradeId,
    types::{
        price::Price,
        quantity::{QUANTITY_MAX, Quantity},
    },
};
use uuid::Uuid;

use super::{
    enums::{Action, WsTopic},
    messages::{OrderBook10Msg, OrderBookMsg, QuoteMsg, TradeBinMsg, TradeMsg},
};
use crate::common::parse::parse_instrument_id;

const BAR_SPEC_1_MINUTE: BarSpecification = BarSpecification {
    step: NonZero::new(1).unwrap(),
    aggregation: BarAggregation::Minute,
    price_type: PriceType::Last,
};
const BAR_SPEC_5_MINUTE: BarSpecification = BarSpecification {
    step: NonZero::new(5).unwrap(),
    aggregation: BarAggregation::Minute,
    price_type: PriceType::Last,
};
const BAR_SPEC_1_HOUR: BarSpecification = BarSpecification {
    step: NonZero::new(1).unwrap(),
    aggregation: BarAggregation::Hour,
    price_type: PriceType::Last,
};
const BAR_SPEC_1_DAY: BarSpecification = BarSpecification {
    step: NonZero::new(1).unwrap(),
    aggregation: BarAggregation::Day,
    price_type: PriceType::Last,
};

#[must_use]
pub fn parse_book_msg_vec(
    data: Vec<OrderBookMsg>,
    action: Action,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Vec<Data> {
    let mut deltas = Vec::with_capacity(data.len());
    for msg in data {
        deltas.push(Data::Delta(parse_book_msg(
            msg,
            &action,
            price_precision,
            ts_init,
        )));
    }
    deltas
}

#[must_use]
pub fn parse_book10_msg_vec(
    data: Vec<OrderBook10Msg>,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Vec<Data> {
    let mut depths = Vec::with_capacity(data.len());
    for msg in data {
        depths.push(Data::Depth10(Box::new(parse_book10_msg(
            msg,
            price_precision,
            ts_init,
        ))));
    }
    depths
}

#[must_use]
pub fn parse_trade_msg_vec(
    data: Vec<TradeMsg>,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Vec<Data> {
    let mut trades = Vec::with_capacity(data.len());
    for msg in data {
        trades.push(Data::Trade(parse_trade_msg(msg, price_precision, ts_init)));
    }
    trades
}

#[must_use]
pub fn parse_trade_bin_msg_vec(
    data: Vec<TradeBinMsg>,
    topic: WsTopic,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Vec<Data> {
    let mut trades = Vec::with_capacity(data.len());
    for msg in data {
        trades.push(Data::Bar(parse_trade_bin_msg(
            msg,
            &topic,
            price_precision,
            ts_init,
        )));
    }
    trades
}

#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn parse_book_msg(
    msg: OrderBookMsg,
    action: &Action,
    price_precision: u8,
    ts_init: UnixNanos,
) -> OrderBookDelta {
    let flags = if action == &Action::Insert {
        RecordFlag::F_SNAPSHOT as u8
    } else {
        0
    };

    let instrument_id = parse_instrument_id(&msg.symbol);
    let action = action.as_book_action();
    let price = Price::new(msg.price, price_precision);
    let side = msg.side.as_order_side();
    let size = parse_quantity(msg.size.unwrap_or(0));
    let order_id = msg.id;
    let order = BookOrder::new(side, price, size, order_id);
    let sequence = 0; // Not available
    let ts_event = UnixNanos::from(msg.transact_time);

    OrderBookDelta::new(
        instrument_id,
        action,
        order,
        flags,
        sequence,
        ts_event,
        ts_init,
    )
}

#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn parse_book10_msg(
    msg: OrderBook10Msg,
    price_precision: u8,
    ts_init: UnixNanos,
) -> OrderBookDepth10 {
    let instrument_id = parse_instrument_id(&msg.symbol);

    let mut bids = Vec::with_capacity(DEPTH10_LEN);
    let mut asks = Vec::with_capacity(DEPTH10_LEN);

    // Initialized with zeros
    let mut bid_counts: [u32; DEPTH10_LEN] = [0; DEPTH10_LEN];
    let mut ask_counts: [u32; DEPTH10_LEN] = [0; DEPTH10_LEN];

    for (i, level) in msg.bids.iter().enumerate() {
        let bid_order = BookOrder::new(
            OrderSide::Buy,
            Price::new(level[0], price_precision),
            Quantity::new(level[1], 0),
            0,
        );

        bids.push(bid_order);
        bid_counts[i] = 1;
    }

    for (i, level) in msg.asks.iter().enumerate() {
        let ask_order = BookOrder::new(
            OrderSide::Sell,
            Price::new(level[0], price_precision),
            Quantity::new(level[1], 0),
            0,
        );

        asks.push(ask_order);
        ask_counts[i] = 1;
    }

    let bids: [BookOrder; DEPTH10_LEN] = bids.try_into().expect("`bids` length should be 10");
    let asks: [BookOrder; DEPTH10_LEN] = asks.try_into().expect("`asks` length should be 10");

    let ts_event = UnixNanos::from(msg.timestamp);

    OrderBookDepth10::new(
        instrument_id,
        bids,
        asks,
        bid_counts,
        ask_counts,
        RecordFlag::F_SNAPSHOT as u8,
        0, // Not applicable for BitMEX L2 books
        ts_event,
        ts_init,
    )
}

#[must_use]
pub fn parse_quote_msg(
    msg: QuoteMsg,
    last_quote: &QuoteTick,
    price_precision: u8,
    ts_init: UnixNanos,
) -> QuoteTick {
    let instrument_id = parse_instrument_id(&msg.symbol);

    let bid_price = match msg.bid_price {
        Some(price) => Price::new(price, price_precision),
        None => last_quote.bid_price,
    };

    let ask_price = match msg.ask_price {
        Some(price) => Price::new(price, price_precision),
        None => last_quote.ask_price,
    };

    let bid_size = match msg.bid_size {
        Some(size) => Quantity::new(std::cmp::min(QUANTITY_MAX as u64, size) as f64, 0),
        None => last_quote.bid_size,
    };

    let ask_size = match msg.ask_size {
        Some(size) => Quantity::new(std::cmp::min(QUANTITY_MAX as u64, size) as f64, 0),
        None => last_quote.ask_size,
    };

    let ts_event = UnixNanos::from(msg.timestamp);

    QuoteTick::new(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
}

#[must_use]
pub fn parse_trade_msg(msg: TradeMsg, price_precision: u8, ts_init: UnixNanos) -> TradeTick {
    let instrument_id = parse_instrument_id(&msg.symbol);
    let price = Price::new(msg.price, price_precision);
    let size = parse_quantity(msg.size);
    let aggressor_side = msg.side.as_aggressor_side();
    let trade_id = TradeId::new(
        msg.trd_match_id
            .map(|uuid| uuid.to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
    );
    let ts_event = UnixNanos::from(msg.timestamp);

    TradeTick::new(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
}

#[must_use]
pub fn parse_trade_bin_msg(
    msg: TradeBinMsg,
    topic: &WsTopic,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Bar {
    let instrument_id = parse_instrument_id(&msg.symbol);
    let spec = bar_spec_from_topic(topic);
    let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

    let open = Price::new(msg.open, price_precision);
    let high = Price::new(msg.high, price_precision);
    let low = Price::new(msg.low, price_precision);
    let close = Price::new(msg.close, price_precision);
    let volume = Quantity::new(msg.volume as f64, 0);
    let ts_event = UnixNanos::from(msg.timestamp);

    Bar::new(bar_type, open, high, low, close, volume, ts_event, ts_init)
}

#[must_use]
pub fn bar_spec_from_topic(topic: &WsTopic) -> BarSpecification {
    match topic {
        WsTopic::TradeBin1m => BAR_SPEC_1_MINUTE,
        WsTopic::TradeBin5m => BAR_SPEC_5_MINUTE,
        WsTopic::TradeBin1h => BAR_SPEC_1_HOUR,
        WsTopic::TradeBin1d => BAR_SPEC_1_DAY,
        _ => panic!("Bar specification not supported for {topic}"),
    }
}

#[must_use]
pub fn topic_from_bar_spec(spec: BarSpecification) -> WsTopic {
    match spec {
        BAR_SPEC_1_MINUTE => WsTopic::TradeBin1m,
        BAR_SPEC_5_MINUTE => WsTopic::TradeBin5m,
        BAR_SPEC_1_HOUR => WsTopic::TradeBin1h,
        BAR_SPEC_1_DAY => WsTopic::TradeBin1d,
        _ => panic!("Bar specification not supported {spec}"),
    }
}

// TODO: Use high-precision when it lands
#[must_use]
pub fn parse_quantity(value: u64) -> Quantity {
    let size_workaround = std::cmp::min(QUANTITY_MAX as u64, value);
    Quantity::new(size_workaround as f64, 0)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::quote::QuoteTick,
        enums::{AggressorSide, BookAction},
        identifiers::InstrumentId,
    };
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_json;

    #[rstest]
    fn test_orderbook_l2_message() {
        let json_data = load_test_json("ws_orderbook_l2.json");

        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let msg: OrderBookMsg = serde_json::from_str(&json_data).unwrap();

        // Test Insert action
        let delta = parse_book_msg(msg.clone(), &Action::Insert, 1, UnixNanos::from(3));
        assert_eq!(delta.instrument_id, instrument_id);
        assert_eq!(delta.order.price, Price::from("98459.9"));
        assert_eq!(delta.order.size, Quantity::from(33000));
        assert_eq!(delta.order.side, OrderSide::Sell);
        assert_eq!(delta.order.order_id, 62400580205);
        assert_eq!(delta.action, BookAction::Add);
        assert_eq!(delta.flags, RecordFlag::F_SNAPSHOT as u8);
        assert_eq!(delta.sequence, 0);
        assert_eq!(delta.ts_event, 1732436782275000000); // 2024-11-24T08:26:22.275Z in nanos
        assert_eq!(delta.ts_init, 3);

        // Test Update action (should have different flags)
        let delta = parse_book_msg(msg, &Action::Update, 1, UnixNanos::from(3));
        assert_eq!(delta.flags, 0);
        assert_eq!(delta.action, BookAction::Update);
    }

    #[rstest]
    fn test_orderbook10_message() {
        let json_data = load_test_json("ws_orderbook_10.json");
        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let msg: OrderBook10Msg = serde_json::from_str(&json_data).unwrap();
        let depth10 = parse_book10_msg(msg, 1, UnixNanos::from(3));

        assert_eq!(depth10.instrument_id, instrument_id);

        // Check first bid level
        assert_eq!(depth10.bids[0].price, Price::from("98490.3"));
        assert_eq!(depth10.bids[0].size, Quantity::from(22400));
        assert_eq!(depth10.bids[0].side, OrderSide::Buy);

        // Check first ask level
        assert_eq!(depth10.asks[0].price, Price::from("98490.4"));
        assert_eq!(depth10.asks[0].size, Quantity::from(17600));
        assert_eq!(depth10.asks[0].side, OrderSide::Sell);

        // Check counts (should be 1 for each populated level)
        assert_eq!(depth10.bid_counts, [1; DEPTH10_LEN]);
        assert_eq!(depth10.ask_counts, [1; DEPTH10_LEN]);

        // Check flags and timestamps
        assert_eq!(depth10.sequence, 0);
        assert_eq!(depth10.flags, RecordFlag::F_SNAPSHOT as u8);
        assert_eq!(depth10.ts_event, 1732436353513000000); // 2024-11-24T08:19:13.513Z in nanos
        assert_eq!(depth10.ts_init, 3);
    }

    #[rstest]
    fn test_quote_message() {
        let json_data = load_test_json("ws_quote.json");

        let instrument_id = InstrumentId::from("BCHUSDT.BITMEX");
        let last_quote = QuoteTick::new(
            instrument_id,
            Price::new(487.50, 2),
            Price::new(488.20, 2),
            Quantity::from(100_000),
            Quantity::from(100_000),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );
        let msg: QuoteMsg = serde_json::from_str(&json_data).unwrap();
        let quote = parse_quote_msg(msg, &last_quote, 2, UnixNanos::from(3));

        assert_eq!(quote.instrument_id, instrument_id);
        assert_eq!(quote.bid_price, Price::from("487.55"));
        assert_eq!(quote.ask_price, Price::from("488.25"));
        assert_eq!(quote.bid_size, Quantity::from(103_000));
        assert_eq!(quote.ask_size, Quantity::from(50_000));
        assert_eq!(quote.ts_event, 1732315465085000000);
        assert_eq!(quote.ts_init, 3);
    }

    #[rstest]
    fn test_trade_message() {
        let json_data = load_test_json("ws_trade.json");

        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let msg: TradeMsg = serde_json::from_str(&json_data).unwrap();
        let trade = parse_trade_msg(msg, 1, UnixNanos::from(3));

        assert_eq!(trade.instrument_id, instrument_id);
        assert_eq!(trade.price, Price::from("98570.9"));
        assert_eq!(trade.size, Quantity::from(100));
        assert_eq!(trade.aggressor_side, AggressorSide::Seller);
        assert_eq!(
            trade.trade_id.to_string(),
            "00000000-006d-1000-0000-000e8737d536"
        );
        assert_eq!(trade.ts_event, 1732436138704000000); // 2024-11-24T08:15:38.704Z in nanos
        assert_eq!(trade.ts_init, 3);
    }

    #[rstest]
    fn test_trade_bin_message() {
        let json_data = load_test_json("ws_trade_bin_1m.json");

        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let topic = WsTopic::TradeBin1m;

        let msg: TradeBinMsg = serde_json::from_str(&json_data).unwrap();
        let bar = parse_trade_bin_msg(msg, &topic, 1, UnixNanos::from(3));

        assert_eq!(bar.instrument_id(), instrument_id);
        assert_eq!(
            bar.bar_type.spec(),
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last)
        );
        assert_eq!(bar.open, Price::from("97550.0"));
        assert_eq!(bar.high, Price::from("97584.4"));
        assert_eq!(bar.low, Price::from("97550.0"));
        assert_eq!(bar.close, Price::from("97570.1"));
        assert_eq!(bar.volume, Quantity::from(84_000));
        assert_eq!(bar.ts_event, 1732392420000000000); // 2024-11-23T20:07:00.000Z in nanos
        assert_eq!(bar.ts_init, 3);
    }
}
