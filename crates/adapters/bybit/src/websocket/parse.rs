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

//! Parsing helpers for Bybit WebSocket payloads.

use std::convert::TryFrom;

use anyhow::{Context, Result, anyhow};
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::TradeId,
    instruments::{Instrument, any::InstrumentAny},
    types::{Price, Quantity},
};

use super::messages::{
    BybitWsOrderbookDepthMsg, BybitWsTickerLinearMsg, BybitWsTickerOptionMsg, BybitWsTrade,
};
use crate::common::parse::{
    parse_millis_timestamp, parse_price_with_precision, parse_quantity_with_precision,
};

/// Parses a WebSocket trade frame into a [`TradeTick`].
pub fn parse_ws_trade_tick(
    trade: &BybitWsTrade,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> Result<TradeTick> {
    let price = parse_price_with_precision(&trade.p, instrument.price_precision(), "trade.p")?;
    let size = parse_quantity_with_precision(&trade.v, instrument.size_precision(), "trade.v")?;
    let aggressor: AggressorSide = trade.taker_side.into();
    let trade_id = TradeId::new_checked(trade.i.as_str())
        .context("invalid trade identifier in Bybit trade message")?;
    let ts_event = parse_millis_i64(trade.t, "trade.T")?;

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("failed to construct TradeTick from Bybit trade message")
}

/// Parses an order book depth message into [`OrderBookDeltas`].
pub fn parse_orderbook_deltas(
    msg: &BybitWsOrderbookDepthMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> Result<OrderBookDeltas> {
    let is_snapshot = msg.msg_type.eq_ignore_ascii_case("snapshot");
    let ts_event = parse_millis_i64(msg.ts, "orderbook.ts")?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };

    let depth = &msg.data;
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let update_id = u64::try_from(depth.u)
        .context("received negative update id in Bybit order book message")?;
    let sequence = u64::try_from(depth.seq)
        .context("received negative sequence in Bybit order book message")?;

    let mut deltas = Vec::new();

    if is_snapshot {
        deltas.push(OrderBookDelta::clear(
            instrument_id,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    let total_levels = depth.b.len() + depth.a.len();
    let mut processed = 0_usize;

    let mut push_level = |values: &[String], side: OrderSide| -> Result<()> {
        let (price, size) = parse_book_level(values, price_precision, size_precision, "orderbook")?;
        let action = if size.is_zero() {
            BookAction::Delete
        } else if is_snapshot {
            BookAction::Add
        } else {
            BookAction::Update
        };

        processed += 1;
        let mut flags = RecordFlag::F_MBP as u8;
        if processed == total_levels {
            flags |= RecordFlag::F_LAST as u8;
        }

        let order = BookOrder::new(side, price, size, update_id);
        let delta = OrderBookDelta::new_checked(
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        )
        .context("failed to construct OrderBookDelta from Bybit book level")?;
        deltas.push(delta);
        Ok(())
    };

    for level in &depth.b {
        push_level(level, OrderSide::Buy)?;
    }
    for level in &depth.a {
        push_level(level, OrderSide::Sell)?;
    }

    if total_levels == 0
        && let Some(last) = deltas.last_mut()
    {
        last.flags |= RecordFlag::F_LAST as u8;
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
        .context("failed to assemble OrderBookDeltas from Bybit message")
}

/// Parses an order book snapshot or delta into a [`QuoteTick`].
pub fn parse_orderbook_quote(
    msg: &BybitWsOrderbookDepthMsg,
    instrument: &InstrumentAny,
    last_quote: Option<&QuoteTick>,
    ts_init: UnixNanos,
) -> Result<QuoteTick> {
    let ts_event = parse_millis_i64(msg.ts, "orderbook.ts")?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let get_best = |levels: &[Vec<String>], label: &str| -> Result<Option<(Price, Quantity)>> {
        if let Some(values) = levels.first() {
            parse_book_level(values, price_precision, size_precision, label).map(Some)
        } else {
            Ok(None)
        }
    };

    let bids = get_best(&msg.data.b, "bid")?;
    let asks = get_best(&msg.data.a, "ask")?;

    let (bid_price, bid_size) = match (bids, last_quote) {
        (Some(level), _) => level,
        (None, Some(prev)) => (prev.bid_price, prev.bid_size),
        (None, None) => {
            return Err(anyhow!(
                "Bybit order book update missing bid levels and no previous quote provided"
            ));
        }
    };

    let (ask_price, ask_size) = match (asks, last_quote) {
        (Some(level), _) => level,
        (None, Some(prev)) => (prev.ask_price, prev.ask_size),
        (None, None) => {
            return Err(anyhow!(
                "Bybit order book update missing ask levels and no previous quote provided"
            ));
        }
    };

    QuoteTick::new_checked(
        instrument.id(),
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
    .context("failed to construct QuoteTick from Bybit order book message")
}

/// Parses a linear or inverse ticker payload into a [`QuoteTick`].
pub fn parse_ticker_linear_quote(
    msg: &BybitWsTickerLinearMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> Result<QuoteTick> {
    let ts_event = parse_millis_i64(msg.ts, "ticker.ts")?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let data = &msg.data;
    let bid_price = data
        .bid1_price
        .as_ref()
        .context("Bybit ticker message missing bid1Price")?
        .as_str();
    let ask_price = data
        .ask1_price
        .as_ref()
        .context("Bybit ticker message missing ask1Price")?
        .as_str();

    let bid_price = parse_price_with_precision(bid_price, price_precision, "ticker.bid1Price")?;
    let ask_price = parse_price_with_precision(ask_price, price_precision, "ticker.ask1Price")?;

    let bid_size_str = data.bid1_size.as_deref().unwrap_or("0");
    let ask_size_str = data.ask1_size.as_deref().unwrap_or("0");

    let bid_size = parse_quantity_with_precision(bid_size_str, size_precision, "ticker.bid1Size")?;
    let ask_size = parse_quantity_with_precision(ask_size_str, size_precision, "ticker.ask1Size")?;

    QuoteTick::new_checked(
        instrument.id(),
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
    .context("failed to construct QuoteTick from Bybit linear ticker message")
}

/// Parses an option ticker payload into a [`QuoteTick`].
pub fn parse_ticker_option_quote(
    msg: &BybitWsTickerOptionMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> Result<QuoteTick> {
    let ts_event = parse_millis_i64(msg.ts, "ticker.ts")?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let data = &msg.data;
    let bid_price =
        parse_price_with_precision(&data.bid_price, price_precision, "ticker.bidPrice")?;
    let ask_price =
        parse_price_with_precision(&data.ask_price, price_precision, "ticker.askPrice")?;
    let bid_size = parse_quantity_with_precision(&data.bid_size, size_precision, "ticker.bidSize")?;
    let ask_size = parse_quantity_with_precision(&data.ask_size, size_precision, "ticker.askSize")?;

    QuoteTick::new_checked(
        instrument.id(),
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
    .context("failed to construct QuoteTick from Bybit option ticker message")
}

fn parse_millis_i64(value: i64, field: &str) -> Result<UnixNanos> {
    if value < 0 {
        Err(anyhow!("{field} must be non-negative, was {value}"))
    } else {
        parse_millis_timestamp(&value.to_string(), field)
    }
}

fn parse_book_level(
    level: &[String],
    price_precision: u8,
    size_precision: u8,
    label: &str,
) -> Result<(Price, Quantity)> {
    let price_str = level
        .first()
        .ok_or_else(|| anyhow!("missing price component in {label} level"))?;
    let size_str = level
        .get(1)
        .ok_or_else(|| anyhow!("missing size component in {label} level"))?;
    let price = parse_price_with_precision(price_str, price_precision, label)?;
    let size = parse_quantity_with_precision(size_str, size_precision, label)?;
    Ok((price, size))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        common::{
            parse::{parse_linear_instrument, parse_option_instrument},
            testing::load_test_json,
        },
        http::models::{BybitInstrumentLinearResponse, BybitInstrumentOptionResponse},
        websocket::messages::{
            BybitWsOrderbookDepthMsg, BybitWsTickerLinearMsg, BybitWsTickerOptionMsg,
            BybitWsTradeMsg,
        },
    };

    const TS: UnixNanos = UnixNanos::new(1_700_000_000_000_000_000);

    use nautilus_model::enums::{AggressorSide, BookAction, OrderSide, RecordFlag};
    use ustr::Ustr;

    use crate::http::models::BybitFeeRate;

    fn sample_fee_rate(
        symbol: &str,
        taker: &str,
        maker: &str,
        base_coin: Option<&str>,
    ) -> BybitFeeRate {
        BybitFeeRate {
            symbol: Ustr::from(symbol),
            taker_fee_rate: taker.to_string(),
            maker_fee_rate: maker.to_string(),
            base_coin: base_coin.map(Ustr::from),
        }
    }

    fn linear_instrument() -> InstrumentAny {
        let json = load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let fee_rate = sample_fee_rate("BTCUSDT", "0.00055", "0.0001", Some("BTC"));
        parse_linear_instrument(instrument, &fee_rate, TS, TS).unwrap()
    }

    fn option_instrument() -> InstrumentAny {
        let json = load_test_json("http_get_instruments_option.json");
        let response: BybitInstrumentOptionResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        parse_option_instrument(instrument, TS, TS).unwrap()
    }

    #[rstest]
    fn parse_ws_trade_into_trade_tick() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_public_trade.json");
        let msg: BybitWsTradeMsg = serde_json::from_str(&json).unwrap();
        let trade = &msg.data[0];

        let tick = parse_ws_trade_tick(trade, &instrument, TS).unwrap();

        assert_eq!(tick.instrument_id, instrument.id());
        assert_eq!(tick.price, instrument.make_price(27451.00));
        assert_eq!(tick.size, instrument.make_qty(0.010, None));
        assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
        assert_eq!(
            tick.trade_id.to_string(),
            "9dc75fca-4bdd-4773-9f78-6f5d7ab2a110"
        );
        assert_eq!(tick.ts_event, UnixNanos::new(1_709_891_679_000_000_000));
    }

    #[rstest]
    fn parse_orderbook_snapshot_into_deltas() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_orderbook_snapshot.json");
        let msg: BybitWsOrderbookDepthMsg = serde_json::from_str(&json).unwrap();

        let deltas = parse_orderbook_deltas(&msg, &instrument, TS).unwrap();

        assert_eq!(deltas.instrument_id, instrument.id());
        assert_eq!(deltas.deltas.len(), 5);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(
            deltas.deltas[1].order.price,
            instrument.make_price(27450.00)
        );
        assert_eq!(
            deltas.deltas[1].order.size,
            instrument.make_qty(0.500, None)
        );
        let last = deltas.deltas.last().unwrap();
        assert_eq!(last.order.side, OrderSide::Sell);
        assert_eq!(last.order.price, instrument.make_price(27451.50));
        assert_eq!(
            last.flags & RecordFlag::F_LAST as u8,
            RecordFlag::F_LAST as u8
        );
    }

    #[rstest]
    fn parse_orderbook_delta_marks_actions() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_orderbook_delta.json");
        let msg: BybitWsOrderbookDepthMsg = serde_json::from_str(&json).unwrap();

        let deltas = parse_orderbook_deltas(&msg, &instrument, TS).unwrap();

        assert_eq!(deltas.deltas.len(), 2);
        let bid = &deltas.deltas[0];
        assert_eq!(bid.action, BookAction::Update);
        assert_eq!(bid.order.side, OrderSide::Buy);
        assert_eq!(bid.order.size, instrument.make_qty(0.400, None));

        let ask = &deltas.deltas[1];
        assert_eq!(ask.action, BookAction::Delete);
        assert_eq!(ask.order.side, OrderSide::Sell);
        assert_eq!(ask.order.size, instrument.make_qty(0.0, None));
        assert_eq!(
            ask.flags & RecordFlag::F_LAST as u8,
            RecordFlag::F_LAST as u8
        );
    }

    #[rstest]
    fn parse_orderbook_quote_produces_top_of_book() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_orderbook_snapshot.json");
        let msg: BybitWsOrderbookDepthMsg = serde_json::from_str(&json).unwrap();

        let quote = parse_orderbook_quote(&msg, &instrument, None, TS).unwrap();

        assert_eq!(quote.instrument_id, instrument.id());
        assert_eq!(quote.bid_price, instrument.make_price(27450.00));
        assert_eq!(quote.bid_size, instrument.make_qty(0.500, None));
        assert_eq!(quote.ask_price, instrument.make_price(27451.00));
        assert_eq!(quote.ask_size, instrument.make_qty(0.750, None));
    }

    #[rstest]
    fn parse_orderbook_quote_with_delta_updates_sizes() {
        let instrument = linear_instrument();
        let snapshot: BybitWsOrderbookDepthMsg =
            serde_json::from_str(&load_test_json("ws_orderbook_snapshot.json")).unwrap();
        let base_quote = parse_orderbook_quote(&snapshot, &instrument, None, TS).unwrap();

        let delta: BybitWsOrderbookDepthMsg =
            serde_json::from_str(&load_test_json("ws_orderbook_delta.json")).unwrap();
        let updated = parse_orderbook_quote(&delta, &instrument, Some(&base_quote), TS).unwrap();

        assert_eq!(updated.bid_price, instrument.make_price(27450.00));
        assert_eq!(updated.bid_size, instrument.make_qty(0.400, None));
        assert_eq!(updated.ask_price, instrument.make_price(27451.00));
        assert_eq!(updated.ask_size, instrument.make_qty(0.0, None));
    }

    #[rstest]
    fn parse_linear_ticker_quote_to_quote_tick() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_ticker_linear.json");
        let msg: BybitWsTickerLinearMsg = serde_json::from_str(&json).unwrap();

        let quote = parse_ticker_linear_quote(&msg, &instrument, TS).unwrap();

        assert_eq!(quote.instrument_id, instrument.id());
        assert_eq!(quote.bid_price, instrument.make_price(17215.50));
        assert_eq!(quote.ask_price, instrument.make_price(17216.00));
        assert_eq!(quote.bid_size, instrument.make_qty(84.489, None));
        assert_eq!(quote.ask_size, instrument.make_qty(83.020, None));
        assert_eq!(quote.ts_event, UnixNanos::new(1_673_272_861_686_000_000));
        assert_eq!(quote.ts_init, TS);
    }

    #[rstest]
    fn parse_option_ticker_quote_to_quote_tick() {
        let instrument = option_instrument();
        let json = load_test_json("ws_ticker_option.json");
        let msg: BybitWsTickerOptionMsg = serde_json::from_str(&json).unwrap();

        let quote = parse_ticker_option_quote(&msg, &instrument, TS).unwrap();

        assert_eq!(quote.instrument_id, instrument.id());
        assert_eq!(quote.bid_price, instrument.make_price(0.0));
        assert_eq!(quote.ask_price, instrument.make_price(10.0));
        assert_eq!(quote.bid_size, instrument.make_qty(0.0, None));
        assert_eq!(quote.ask_size, instrument.make_qty(5.1, None));
        assert_eq!(quote.ts_event, UnixNanos::new(1_672_917_511_074_000_000));
        assert_eq!(quote.ts_init, TS);
    }
}
