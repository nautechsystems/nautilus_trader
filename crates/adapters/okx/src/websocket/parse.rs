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

use ahash::AHashMap;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, Data, IndexPriceUpdate, MarkPriceUpdate,
        OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API, QuoteTick, TradeTick,
    },
    enums::{
        AggregationSource, AggressorSide, BookAction, LiquiditySide, OrderSide, OrderStatus,
        OrderType, RecordFlag, TimeInForce,
    },
    identifiers::{AccountId, InstrumentId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money},
};
use ustr::Ustr;

use super::messages::{
    OKXBookMsg, OKXCandleMsg, OKXIndexPriceMsg, OKXMarkPriceMsg, OKXOrderMsg, OKXTickerMsg,
    OKXTradeMsg, OrderBookEntry,
};
use crate::{
    common::{
        enums::{OKXBookAction, OKXCandleConfirm, OKXOrderStatus, OKXOrderType},
        parse::{
            parse_client_order_id, parse_millisecond_timestamp, parse_order_side, parse_price,
            parse_quantity,
        },
    },
    websocket::messages::ExecutionReport,
};

pub fn parse_book_msg_vec(
    data: Vec<OKXBookMsg>,
    instrument_id: &InstrumentId,
    price_precision: u8,
    size_precision: u8,
    action: OKXBookAction,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Data>> {
    let mut deltas = Vec::with_capacity(data.len());

    for msg in data {
        let deltas_api = OrderBookDeltas_API::new(parse_book_msg(
            &msg,
            *instrument_id,
            price_precision,
            size_precision,
            &action,
            ts_init,
        )?);
        deltas.push(Data::Deltas(deltas_api));
    }

    Ok(deltas)
}

pub fn parse_ticker_msg_vec(
    data: serde_json::Value,
    instrument_id: &InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Data>> {
    let msgs: Vec<OKXTickerMsg> = serde_json::from_value(data)?;
    let mut quotes = Vec::with_capacity(msgs.len());

    for msg in msgs {
        let quote = parse_ticker_msg(
            &msg,
            *instrument_id,
            price_precision,
            size_precision,
            ts_init,
        )?;
        quotes.push(Data::Quote(quote));
    }

    Ok(quotes)
}

pub fn parse_quote_msg_vec(
    data: serde_json::Value,
    instrument_id: &InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Data>> {
    let msgs: Vec<OKXBookMsg> = serde_json::from_value(data)?;
    let mut quotes = Vec::with_capacity(msgs.len());

    for msg in msgs {
        let quote = parse_quote_msg(
            &msg,
            *instrument_id,
            price_precision,
            size_precision,
            ts_init,
        )?;
        quotes.push(Data::Quote(quote));
    }

    Ok(quotes)
}

pub fn parse_trade_msg_vec(
    data: serde_json::Value,
    instrument_id: &InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Data>> {
    let msgs: Vec<OKXTradeMsg> = serde_json::from_value(data)?;
    let mut trades = Vec::with_capacity(msgs.len());

    for msg in msgs {
        let trade = parse_trade_msg(
            &msg,
            *instrument_id,
            price_precision,
            size_precision,
            ts_init,
        )?;
        trades.push(Data::Trade(trade));
    }

    Ok(trades)
}

pub fn parse_mark_price_msg_vec(
    data: serde_json::Value,
    instrument_id: &InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Data>> {
    let msgs: Vec<OKXMarkPriceMsg> = serde_json::from_value(data)?;
    let mut updates = Vec::with_capacity(msgs.len());

    for msg in msgs {
        let update = parse_mark_price_msg(&msg, *instrument_id, price_precision, ts_init)?;
        updates.push(Data::MarkPriceUpdate(update));
    }

    Ok(updates)
}

pub fn parse_index_price_msg_vec(
    data: serde_json::Value,
    instrument_id: &InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Data>> {
    let msgs: Vec<OKXIndexPriceMsg> = serde_json::from_value(data)?;
    let mut updates = Vec::with_capacity(msgs.len());

    for msg in msgs {
        let update = parse_index_price_msg(&msg, *instrument_id, price_precision, ts_init)?;
        updates.push(Data::IndexPriceUpdate(update));
    }

    Ok(updates)
}

pub fn parse_candle_msg_vec(
    data: serde_json::Value,
    instrument_id: &InstrumentId,
    price_precision: u8,
    size_precision: u8,
    spec: BarSpecification,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Data>> {
    let msgs: Vec<OKXCandleMsg> = serde_json::from_value(data)?;
    let bar_type = BarType::new(*instrument_id, spec, AggregationSource::External);
    let mut bars = Vec::with_capacity(msgs.len());

    for msg in msgs {
        // Only process completed candles to avoid duplicate/partial bars
        if msg.confirm == OKXCandleConfirm::Closed {
            let bar = parse_candle_msg(&msg, bar_type, price_precision, size_precision, ts_init)?;
            bars.push(Data::Bar(bar));
        }
    }

    Ok(bars)
}

pub fn parse_book_msg(
    msg: &OKXBookMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    action: &OKXBookAction,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let flags = if action == &OKXBookAction::Snapshot {
        RecordFlag::F_SNAPSHOT as u8
    } else {
        0
    };
    let ts_event = parse_millisecond_timestamp(msg.ts);

    let mut deltas = Vec::with_capacity(msg.asks.len() + msg.bids.len());

    for bid in &msg.bids {
        let book_action = match action {
            OKXBookAction::Snapshot => BookAction::Add,
            _ => match bid.size.as_str() {
                "0" => BookAction::Delete,
                _ => BookAction::Update,
            },
        };
        let price = parse_price(&bid.price, price_precision)?;
        let size = parse_quantity(&bid.size, size_precision)?;
        let order_id = 0; // TBD
        let order = BookOrder::new(OrderSide::Buy, price, size, order_id);
        let delta = OrderBookDelta::new(
            instrument_id,
            book_action,
            order,
            flags,
            msg.seq_id,
            ts_event,
            ts_init,
        );
        deltas.push(delta)
    }

    for ask in &msg.asks {
        let book_action = match action {
            OKXBookAction::Snapshot => BookAction::Add,
            _ => match ask.size.as_str() {
                "0" => BookAction::Delete,
                _ => BookAction::Update,
            },
        };
        let price = parse_price(&ask.price, price_precision)?;
        let size = parse_quantity(&ask.size, size_precision)?;
        let order_id = 0; // TBD
        let order = BookOrder::new(OrderSide::Sell, price, size, order_id);
        let delta = OrderBookDelta::new(
            instrument_id,
            book_action,
            order,
            flags,
            msg.seq_id,
            ts_event,
            ts_init,
        );
        deltas.push(delta)
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
}

pub fn parse_quote_msg(
    msg: &OKXBookMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let best_bid: &OrderBookEntry = &msg.bids[0];
    let best_ask: &OrderBookEntry = &msg.asks[0];

    let bid_price = parse_price(&best_bid.price, price_precision)?;
    let ask_price = parse_price(&best_ask.price, price_precision)?;
    let bid_size = parse_quantity(&best_bid.size, size_precision)?;
    let ask_size = parse_quantity(&best_ask.size, size_precision)?;
    let ts_event = parse_millisecond_timestamp(msg.ts);

    QuoteTick::new_checked(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
}

// TODO: Parsing depth10 messages
// #[must_use]
// #[allow(clippy::too_many_arguments)]
// pub fn parse_book10_msg(
//     msg: OrderBook10Msg,
//     price_precision: u8,
//     ts_init: UnixNanos,
// ) -> OrderBookDepth10 {
//     let instrument_id = parse_instrument_id(&msg.symbol);
//
//     let mut bids = Vec::with_capacity(DEPTH10_LEN);
//     let mut asks = Vec::with_capacity(DEPTH10_LEN);
//
//     // Initialized with zeros
//     let mut bid_counts: [u32; DEPTH10_LEN] = [0; DEPTH10_LEN];
//     let mut ask_counts: [u32; DEPTH10_LEN] = [0; DEPTH10_LEN];
//
//     for (i, level) in msg.bids.iter().enumerate() {
//         let bid_order = BookOrder::new(
//             OrderSide::Buy,
//             Price::new(level[0], price_precision),
//             Quantity::new(level[1], 0),
//             0,
//         );
//
//         bids.push(bid_order);
//         bid_counts[i] = 1;
//     }
//
//     for (i, level) in msg.asks.iter().enumerate() {
//         let ask_order = BookOrder::new(
//             OrderSide::Sell,
//             Price::new(level[0], price_precision),
//             Quantity::new(level[1], 0),
//             0,
//         );
//
//         asks.push(ask_order);
//         ask_counts[i] = 1;
//     }
//
//     let bids: [BookOrder; DEPTH10_LEN] = bids.try_into().expect("`bids` length should be 10");
//     let asks: [BookOrder; DEPTH10_LEN] = asks.try_into().expect("`asks` length should be 10");
//
//     let ts_event = UnixNanos::from(msg.timestamp);
//
//     OrderBookDepth10::new(
//         instrument_id,
//         bids,
//         asks,
//         bid_counts,
//         ask_counts,
//         RecordFlag::F_SNAPSHOT.value(),
//         0, // Not applicable for OKX L2 books
//         ts_event,
//         ts_init,
//     )
// }

pub fn parse_ticker_msg(
    msg: &OKXTickerMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let bid_price = parse_price(&msg.bid_px, price_precision)?;
    let ask_price = parse_price(&msg.ask_px, price_precision)?;
    let bid_size = parse_quantity(&msg.bid_sz, size_precision)?;
    let ask_size = parse_quantity(&msg.ask_sz, size_precision)?;
    let ts_event = parse_millisecond_timestamp(msg.ts);

    QuoteTick::new_checked(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
}

pub fn parse_trade_msg(
    msg: &OKXTradeMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(&msg.px, price_precision)?;
    let size = parse_quantity(&msg.sz, size_precision)?;
    let aggressor_side = AggressorSide::from(msg.side.clone());
    let trade_id = TradeId::new(&msg.trade_id);
    let ts_event = parse_millisecond_timestamp(msg.ts);

    TradeTick::new_checked(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
}

pub fn parse_mark_price_msg(
    msg: &OKXMarkPriceMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<MarkPriceUpdate> {
    let price = parse_price(&msg.mark_px, price_precision)?;
    let ts_event = parse_millisecond_timestamp(msg.ts);

    Ok(MarkPriceUpdate::new(
        instrument_id,
        price,
        ts_event,
        ts_init,
    ))
}

pub fn parse_index_price_msg(
    msg: &OKXIndexPriceMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<IndexPriceUpdate> {
    let price = parse_price(&msg.idx_px, price_precision)?;
    let ts_event = parse_millisecond_timestamp(msg.ts);

    Ok(IndexPriceUpdate::new(
        instrument_id,
        price,
        ts_event,
        ts_init,
    ))
}

pub fn parse_candle_msg(
    msg: &OKXCandleMsg,
    bar_type: BarType,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let open = parse_price(&msg.o, price_precision)?;
    let high = parse_price(&msg.h, price_precision)?;
    let low = parse_price(&msg.l, price_precision)?;
    let close = parse_price(&msg.c, price_precision)?;
    let volume = parse_quantity(&msg.vol, size_precision)?;
    let ts_event = parse_millisecond_timestamp(msg.ts);

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
}

/// Parses OKX WebSocket order messages into Nautilus order events.
pub fn parse_order_msg_vec(
    data: Vec<OKXOrderMsg>,
    account_id: AccountId,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<ExecutionReport>> {
    let mut order_reports = Vec::with_capacity(data.len());

    for msg in data {
        let inst = instruments
            .get(&msg.inst_id)
            .expect("No instrument found for inst_id");

        let result = match &msg.state {
            OKXOrderStatus::Filled | OKXOrderStatus::PartiallyFilled => {
                parse_fill_report(&msg, inst, account_id, ts_init).map(ExecutionReport::Fill)
            }
            _ => parse_order_status_report(&msg, inst, account_id, ts_init)
                .map(ExecutionReport::Order),
        };

        match result {
            Ok(report) => order_reports.push(report),
            Err(e) => tracing::error!("Failed to parse execution report from message: {e}"),
        }
    }

    Ok(order_reports)
}

/// Parses an OrderStatusReport from an OKX order message.
pub fn parse_order_status_report(
    msg: &OKXOrderMsg,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let client_order_id = parse_client_order_id(&msg.cl_ord_id);
    let venue_order_id = VenueOrderId::new(msg.ord_id);
    let order_side = parse_order_side(&Some(msg.side.clone()));

    let okx_order_type = match msg.ord_type.as_str() {
        "market" => OKXOrderType::Market,
        "limit" => OKXOrderType::Limit,
        "post_only" => OKXOrderType::PostOnly,
        "fok" => OKXOrderType::Fok,
        "ioc" => OKXOrderType::Ioc,
        "optimal_limit_ioc" => OKXOrderType::OptimalLimitIoc,
        "mmp" => OKXOrderType::Mmp,
        "mmp_and_post_only" => OKXOrderType::MmpAndPostOnly,
        _ => OKXOrderType::Limit, // Default fallback
    };
    let order_type: OrderType = okx_order_type.clone().into();

    let size_precision = instrument.size_precision();
    let quantity = parse_quantity(&msg.sz, size_precision)?;
    let filled_qty = parse_quantity(&msg.acc_fill_sz.clone().unwrap_or_default(), size_precision)?;
    let order_status: OrderStatus = msg.state.clone().into();

    let ts_accepted = parse_millisecond_timestamp(msg.c_time);
    let ts_last = parse_millisecond_timestamp(msg.u_time);

    let time_in_force = match okx_order_type {
        OKXOrderType::Fok => TimeInForce::Fok,
        OKXOrderType::Ioc | OKXOrderType::OptimalLimitIoc => TimeInForce::Ioc,
        _ => TimeInForce::Gtc,
    };

    let report = OrderStatusReport::new(
        account_id,
        instrument.id(),
        client_order_id,
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_accepted,
        ts_init,
        ts_last,
        None, // Generate UUID4 automatically
    );

    Ok(report)
}

/// Parses a FillReport from an OKX order message.
pub fn parse_fill_report(
    msg: &OKXOrderMsg,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let client_order_id = parse_client_order_id(&msg.cl_ord_id);
    let venue_order_id = VenueOrderId::new(msg.ord_id);
    let trade_id = TradeId::from(msg.trade_id.as_str());
    let order_side = parse_order_side(&Some(msg.side.clone()));

    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let last_px = parse_price(&msg.fill_px, price_precision)?;
    let last_qty = parse_quantity(&msg.fill_sz, size_precision)?;
    let fee_f64 = msg.fee.as_deref().unwrap_or("0").parse::<f64>()?;
    let commission = Money::new(-fee_f64, Currency::from(&msg.fee_ccy));
    let liquidity_side: LiquiditySide = LiquiditySide::from(msg.exec_type.clone());

    let ts_event = parse_millisecond_timestamp(msg.fill_time);

    let report = FillReport::new(
        account_id,
        instrument.id(),
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        client_order_id,
        None,
        ts_event,
        ts_init,
        None, // Generate UUID4 automatically
    );

    Ok(report)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::bar::BAR_SPEC_1_DAY_LAST,
        enums::AggressorSide,
        identifiers::{ClientOrderId, InstrumentId},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::{common::testing::load_test_json, websocket::messages::OKXWebSocketEvent};

    #[rstest]
    fn test_parse_books_snapshot() {
        let json_data = load_test_json("ws_books_snapshot.json");
        let msg: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let (okx_books, action): (Vec<OKXBookMsg>, OKXBookAction) = match msg {
            OKXWebSocketEvent::BookData { data, action, .. } => (data, action),
            _ => panic!("Expected a `BookData` variant"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let deltas = parse_book_msg(
            &okx_books[0],
            instrument_id,
            2,
            1,
            &action,
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(deltas.instrument_id, instrument_id);
        assert_eq!(deltas.deltas.len(), 16);
        assert_eq!(deltas.flags, 32);
        assert_eq!(deltas.sequence, 123456);
        assert_eq!(deltas.ts_event, UnixNanos::from(1597026383085000000));
        assert_eq!(deltas.ts_init, UnixNanos::default());
        // TODO: Complete parsing testing of remaining fields
    }

    #[rstest]
    fn test_parse_books_update() {
        let json_data = load_test_json("ws_books_update.json");
        let msg: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let (okx_books, action): (Vec<OKXBookMsg>, OKXBookAction) = match msg {
            OKXWebSocketEvent::BookData { data, action, .. } => (data, action),
            _ => panic!("Expected a `BookData` variant"),
        };

        let deltas = parse_book_msg(
            &okx_books[0],
            instrument_id,
            2,
            1,
            &action,
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(deltas.instrument_id, instrument_id);
        assert_eq!(deltas.deltas.len(), 16);
        assert_eq!(deltas.flags, 0);
        assert_eq!(deltas.sequence, 123457);
        assert_eq!(deltas.ts_event, UnixNanos::from(1597026383085000000));
        assert_eq!(deltas.ts_init, UnixNanos::default());
        // TODO: Complete parsing testing of remaining fields
    }

    #[rstest]
    fn test_parse_tickers() {
        let json_data = load_test_json("ws_tickers.json");
        let msg: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let okx_tickers: Vec<OKXTickerMsg> = match msg {
            OKXWebSocketEvent::Data { data, .. } => serde_json::from_value(data).unwrap(),
            _ => panic!("Expected a `Data` variant"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let trade =
            parse_ticker_msg(&okx_tickers[0], instrument_id, 2, 1, UnixNanos::default()).unwrap();

        assert_eq!(trade.instrument_id, InstrumentId::from("BTC-USDT.OKX"));
        assert_eq!(trade.bid_price, Price::from("8888.88"));
        assert_eq!(trade.ask_price, Price::from("9999.99"));
        assert_eq!(trade.bid_size, Quantity::from(5));
        assert_eq!(trade.ask_size, Quantity::from(11));
        assert_eq!(trade.ts_event, UnixNanos::from(1597026383085000000));
        assert_eq!(trade.ts_init, UnixNanos::default());
    }

    #[rstest]
    fn test_parse_quotes() {
        let json_data = load_test_json("ws_bbo_tbt.json");
        let msg: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let okx_quotes: Vec<OKXBookMsg> = match msg {
            OKXWebSocketEvent::Data { data, .. } => serde_json::from_value(data).unwrap(),
            _ => panic!("Expected a `Data` variant"),
        };
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");

        let quote =
            parse_quote_msg(&okx_quotes[0], instrument_id, 2, 1, UnixNanos::default()).unwrap();

        assert_eq!(quote.instrument_id, InstrumentId::from("BTC-USDT.OKX"));
        assert_eq!(quote.bid_price, Price::from("8476.97"));
        assert_eq!(quote.ask_price, Price::from("8476.98"));
        assert_eq!(quote.bid_size, Quantity::from(256));
        assert_eq!(quote.ask_size, Quantity::from(415));
        assert_eq!(quote.ts_event, UnixNanos::from(1597026383085000000));
        assert_eq!(quote.ts_init, UnixNanos::default());
    }

    #[rstest]
    fn test_parse_trades() {
        let json_data = load_test_json("ws_trades.json");
        let msg: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let okx_trades: Vec<OKXTradeMsg> = match msg {
            OKXWebSocketEvent::Data { data, .. } => serde_json::from_value(data).unwrap(),
            _ => panic!("Expected a `Data` variant"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let trade =
            parse_trade_msg(&okx_trades[0], instrument_id, 1, 8, UnixNanos::default()).unwrap();

        assert_eq!(trade.instrument_id, InstrumentId::from("BTC-USDT.OKX"));
        assert_eq!(trade.price, Price::from("42219.9"));
        assert_eq!(trade.size, Quantity::from("0.12060306"));
        assert_eq!(trade.aggressor_side, AggressorSide::Buyer);
        assert_eq!(trade.trade_id, TradeId::from("130639474"));
        assert_eq!(trade.ts_event, UnixNanos::from(1630048897897000000));
        assert_eq!(trade.ts_init, UnixNanos::default());
    }

    #[rstest]
    fn test_parse_candle() {
        let json_data = load_test_json("ws_candle.json");
        let msg: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let okx_candles: Vec<OKXCandleMsg> = match msg {
            OKXWebSocketEvent::Data { data, .. } => serde_json::from_value(data).unwrap(),
            _ => panic!("Expected a `Data` variant"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let bar_type = BarType::new(
            instrument_id,
            BAR_SPEC_1_DAY_LAST,
            AggregationSource::External,
        );
        let bar = parse_candle_msg(&okx_candles[0], bar_type, 2, 0, UnixNanos::default()).unwrap();

        assert_eq!(bar.bar_type, bar_type);
        assert_eq!(bar.open, Price::from("8533.02"));
        assert_eq!(bar.high, Price::from("8553.74"));
        assert_eq!(bar.low, Price::from("8527.17"));
        assert_eq!(bar.close, Price::from("8548.26"));
        assert_eq!(bar.volume, Quantity::from(45247));
        assert_eq!(bar.ts_event, UnixNanos::from(1597026383085000000));
        assert_eq!(bar.ts_init, UnixNanos::default());
    }

    #[rstest]
    fn test_parse_book_vec() {
        let json_data = load_test_json("ws_books_snapshot.json");
        let event: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let (msgs, action): (Vec<OKXBookMsg>, OKXBookAction) = match event {
            OKXWebSocketEvent::BookData { data, action, .. } => (data, action),
            _ => panic!("Expected BookData"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let deltas_vec =
            parse_book_msg_vec(msgs, &instrument_id, 8, 1, action, UnixNanos::default()).unwrap();

        assert_eq!(deltas_vec.len(), 1);

        if let Data::Deltas(d) = &deltas_vec[0] {
            assert_eq!(d.sequence, 123456);
        } else {
            panic!("Expected Deltas");
        }
    }

    #[rstest]
    fn test_parse_ticker_vec() {
        let json_data = load_test_json("ws_tickers.json");
        let event: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let data_val: serde_json::Value = match event {
            OKXWebSocketEvent::Data { data, .. } => data,
            _ => panic!("Expected Data"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let quotes_vec =
            parse_ticker_msg_vec(data_val, &instrument_id, 8, 1, UnixNanos::default()).unwrap();

        assert_eq!(quotes_vec.len(), 1);

        if let Data::Quote(q) = &quotes_vec[0] {
            assert_eq!(q.bid_price, Price::from("8888.88000000"));
            assert_eq!(q.ask_price, Price::from("9999.99"));
        } else {
            panic!("Expected Quote");
        }
    }

    #[rstest]
    fn test_parse_trade_vec() {
        let json_data = load_test_json("ws_trades.json");
        let event: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let data_val: serde_json::Value = match event {
            OKXWebSocketEvent::Data { data, .. } => data,
            _ => panic!("Expected Data"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let trades_vec =
            parse_trade_msg_vec(data_val, &instrument_id, 8, 1, UnixNanos::default()).unwrap();

        assert_eq!(trades_vec.len(), 1);

        if let Data::Trade(t) = &trades_vec[0] {
            assert_eq!(t.trade_id, TradeId::new("130639474"));
        } else {
            panic!("Expected Trade");
        }
    }

    #[rstest]
    fn test_parse_candle_vec() {
        let json_data = load_test_json("ws_candle.json");
        let event: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let data_val: serde_json::Value = match event {
            OKXWebSocketEvent::Data { data, .. } => data,
            _ => panic!("Expected Data"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let bars_vec = parse_candle_msg_vec(
            data_val,
            &instrument_id,
            2,
            1,
            BAR_SPEC_1_DAY_LAST,
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(bars_vec.len(), 1);

        if let Data::Bar(b) = &bars_vec[0] {
            assert_eq!(b.open, Price::from("8533.02"));
        } else {
            panic!("Expected Bar");
        }
    }

    #[rstest]
    fn test_parse_book_message() {
        use ustr::Ustr;

        use crate::websocket::{
            enums::OKXWsChannel,
            messages::{OKXWebSocketArg, OKXWebSocketEvent},
        };

        let json_data = load_test_json("ws_bbo_tbt.json");
        let msg: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let (okx_books, arg): (Vec<OKXBookMsg>, OKXWebSocketArg) = match msg {
            OKXWebSocketEvent::Data { data, arg, .. } => {
                (serde_json::from_value(data).unwrap(), arg)
            }
            _ => panic!("Expected a `Data` variant"),
        };

        assert_eq!(arg.channel, OKXWsChannel::BboTbt);
        assert_eq!(arg.inst_id.as_ref().unwrap(), &Ustr::from("BTC-USDT"));
        assert_eq!(arg.inst_type, None);
        assert_eq!(okx_books.len(), 1);

        let book_msg = &okx_books[0];

        // Check asks
        assert_eq!(book_msg.asks.len(), 1);
        let ask = &book_msg.asks[0];
        assert_eq!(ask.price, "8476.98");
        assert_eq!(ask.size, "415");
        assert_eq!(ask.liquidated_orders_count, "0");
        assert_eq!(ask.orders_count, "13");

        // Check bids
        assert_eq!(book_msg.bids.len(), 1);
        let bid = &book_msg.bids[0];
        assert_eq!(bid.price, "8476.97");
        assert_eq!(bid.size, "256");
        assert_eq!(bid.liquidated_orders_count, "0");
        assert_eq!(bid.orders_count, "12");
        assert_eq!(book_msg.ts, 1597026383085);
        assert_eq!(book_msg.seq_id, 123456);
        assert_eq!(book_msg.checksum, None);
        assert_eq!(book_msg.prev_seq_id, None);
    }

    #[rstest]
    fn test_parse_ws_account_message() {
        use nautilus_model::identifiers::AccountId;

        // Load test data for WebSocket account message
        let json_data = load_test_json("ws_account.json");
        let accounts: Vec<crate::http::models::OKXAccount> =
            serde_json::from_str(&json_data).unwrap();

        assert_eq!(accounts.len(), 1);
        let account = &accounts[0];

        assert_eq!(account.total_eq, "100.56089404807182");
        assert_eq!(account.details.len(), 3);

        let usdt_detail = &account.details[0];
        assert_eq!(usdt_detail.ccy, "USDT");
        assert_eq!(usdt_detail.avail_bal, "100.52768569797846");
        assert_eq!(usdt_detail.cash_bal, "100.52768569797846");

        let btc_detail = &account.details[1];
        assert_eq!(btc_detail.ccy, "BTC");
        assert_eq!(btc_detail.avail_bal, "0.0000000051");

        let eth_detail = &account.details[2];
        assert_eq!(eth_detail.ccy, "ETH");
        assert_eq!(eth_detail.avail_bal, "0.000000185");

        let account_id = AccountId::new("OKX-001");
        let ts_init = nautilus_core::nanos::UnixNanos::default();
        let account_state = crate::common::parse::parse_account_state(account, account_id, ts_init);

        assert!(account_state.is_ok());
        let state = account_state.unwrap();
        assert_eq!(state.account_id, account_id);
        assert_eq!(state.balances.len(), 3);
    }

    #[rstest]
    fn test_parse_order_msg() {
        use ahash::AHashMap;
        use nautilus_core::nanos::UnixNanos;
        use nautilus_model::{
            identifiers::Symbol,
            instruments::CryptoPerpetual,
            types::{Currency, Price, Quantity},
        };
        use ustr::Ustr;

        let json_data = load_test_json("ws_orders.json");
        let ws_msg: serde_json::Value = serde_json::from_str(&json_data).unwrap();

        let data: Vec<OKXOrderMsg> = serde_json::from_value(ws_msg["data"].clone()).unwrap();

        let account_id = AccountId::new("OKX-001");
        let mut instruments = AHashMap::new();

        // Create a mock instrument for testing
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USDT-SWAP"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false, // is_inverse
            2,     // price_precision
            8,     // size_precision
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None, // multiplier
            None, // lot_size
            None, // max_quantity
            None, // min_quantity
            None, // max_notional
            None, // min_notional
            None, // max_price
            None, // min_price
            None, // margin_init
            None, // margin_maint
            None, // maker_fee
            None, // taker_fee
            UnixNanos::default(),
            UnixNanos::default(),
        );

        instruments.insert(
            Ustr::from("BTC-USDT-SWAP"),
            InstrumentAny::CryptoPerpetual(instrument),
        );

        let ts_init = UnixNanos::default();

        let result = parse_order_msg_vec(data, account_id, &instruments, ts_init);

        assert!(result.is_ok());
        let order_reports = result.unwrap();
        assert_eq!(order_reports.len(), 1);

        // Verify the parsed order report
        let report = &order_reports[0];

        if let ExecutionReport::Fill(fill_report) = report {
            assert_eq!(fill_report.account_id, account_id);
            assert_eq!(fill_report.instrument_id, instrument_id);
            assert_eq!(
                fill_report.client_order_id,
                Some(ClientOrderId::new("001BTCUSDT20250106001"))
            );
            assert_eq!(
                fill_report.venue_order_id,
                VenueOrderId::new("2497956918703120384")
            );
            assert_eq!(fill_report.trade_id, TradeId::from("1518905529"));
            assert_eq!(fill_report.order_side, OrderSide::Buy);
            assert_eq!(fill_report.last_px, Price::from("103698.90"));
            assert_eq!(fill_report.last_qty, Quantity::from("0.03000000"));
            assert_eq!(fill_report.liquidity_side, LiquiditySide::Maker);
        } else {
            panic!("Expected Fill report for filled order");
        }
    }

    #[rstest]
    fn test_parse_order_status_report() {
        use nautilus_core::nanos::UnixNanos;
        use nautilus_model::{
            enums::OrderStatus,
            identifiers::Symbol,
            instruments::CryptoPerpetual,
            types::{Currency, Price, Quantity},
        };

        let json_data = load_test_json("ws_orders.json");
        let ws_msg: serde_json::Value = serde_json::from_str(&json_data).unwrap();
        let data: Vec<OKXOrderMsg> = serde_json::from_value(ws_msg["data"].clone()).unwrap();
        let order_msg = &data[0];

        let account_id = AccountId::new("OKX-001");
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USDT-SWAP"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false, // is_inverse
            2,     // price_precision
            8,     // size_precision
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let ts_init = UnixNanos::default();

        let result = parse_order_status_report(
            order_msg,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            ts_init,
        );

        assert!(result.is_ok());
        let order_status_report = result.unwrap();

        assert_eq!(order_status_report.account_id, account_id);
        assert_eq!(order_status_report.instrument_id, instrument_id);
        assert_eq!(
            order_status_report.client_order_id,
            Some(ClientOrderId::new("001BTCUSDT20250106001"))
        );
        assert_eq!(
            order_status_report.venue_order_id,
            VenueOrderId::new("2497956918703120384")
        );
        assert_eq!(order_status_report.order_side, OrderSide::Buy);
        assert_eq!(order_status_report.order_status, OrderStatus::Filled);
        assert_eq!(order_status_report.quantity, Quantity::from("0.03000000"));
        assert_eq!(order_status_report.filled_qty, Quantity::from("0.03000000"));
    }

    #[rstest]
    fn test_parse_fill_report() {
        use nautilus_core::nanos::UnixNanos;
        use nautilus_model::{
            identifiers::Symbol,
            instruments::CryptoPerpetual,
            types::{Currency, Price, Quantity},
        };

        let json_data = load_test_json("ws_orders.json");
        let ws_msg: serde_json::Value = serde_json::from_str(&json_data).unwrap();
        let data: Vec<OKXOrderMsg> = serde_json::from_value(ws_msg["data"].clone()).unwrap();
        let order_msg = &data[0];

        let account_id = AccountId::new("OKX-001");
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USDT-SWAP"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false, // is_inverse
            2,     // price_precision
            8,     // size_precision
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let ts_init = UnixNanos::default();

        let result = parse_fill_report(
            order_msg,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            ts_init,
        );

        assert!(result.is_ok());
        let fill_report = result.unwrap();

        assert_eq!(fill_report.account_id, account_id);
        assert_eq!(fill_report.instrument_id, instrument_id);
        assert_eq!(
            fill_report.client_order_id,
            Some(ClientOrderId::new("001BTCUSDT20250106001"))
        );
        assert_eq!(
            fill_report.venue_order_id,
            VenueOrderId::new("2497956918703120384")
        );
        assert_eq!(fill_report.trade_id, TradeId::from("1518905529"));
        assert_eq!(fill_report.order_side, OrderSide::Buy);
        assert_eq!(fill_report.last_px, Price::from("103698.90"));
        assert_eq!(fill_report.last_qty, Quantity::from("0.03000000"));
        assert_eq!(fill_report.liquidity_side, LiquiditySide::Maker);
    }
}
