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

//! Parsing helpers for Lighter WebSocket payloads.

use std::{collections::HashMap, str::FromStr};

use anyhow::{Context, Result};
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate, TradeTick},
    enums::AggressorSide,
    identifiers::TradeId,
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};

use crate::{
    data::order_book::depth_to_deltas_and_quote,
    websocket::messages::{LighterTrade, NautilusWsMessage, WsMarketStatsMessage, WsMessage},
};

/// Parse a raw WebSocket message into zero or more Nautilus data events.
pub fn parse_ws_message(
    message: WsMessage,
    instruments: &HashMap<u32, InstrumentAny>,
    ts_init: UnixNanos,
) -> Result<Vec<NautilusWsMessage>> {
    match message {
        WsMessage::Connected { .. } => Ok(Vec::new()),
        WsMessage::OrderBookSnapshot(msg) | WsMessage::OrderBookUpdate(msg) => {
            parse_order_book(msg, instruments, ts_init)
        }
        WsMessage::TradesSnapshot(msg) | WsMessage::TradesUpdate(msg) => {
            parse_trades(msg, instruments, ts_init)
        }
        WsMessage::MarketStats(msg) => parse_market_stats(*msg, instruments, ts_init),
    }
}

fn parse_order_book(
    msg: crate::websocket::messages::WsOrderBookMessage,
    instruments: &HashMap<u32, InstrumentAny>,
    ts_init: UnixNanos,
) -> Result<Vec<NautilusWsMessage>> {
    let market_index = parse_market_index(&msg.channel)
        .with_context(|| format!("missing market index in channel {}", msg.channel))?;
    let instrument = instruments
        .get(&market_index)
        .context("unknown instrument")?;
    let ts_event = parse_timestamp(msg.timestamp, ts_init);

    let (deltas, quote) =
        depth_to_deltas_and_quote(&msg.order_book, instrument, ts_event, ts_init)?;

    let mut events = Vec::with_capacity(2);
    events.push(NautilusWsMessage::Deltas(deltas));
    if let Some(q) = quote {
        events.push(NautilusWsMessage::Quote(q));
    }

    Ok(events)
}

fn parse_trades(
    msg: crate::websocket::messages::WsTradesMessage,
    instruments: &HashMap<u32, InstrumentAny>,
    ts_init: UnixNanos,
) -> Result<Vec<NautilusWsMessage>> {
    let market_index = parse_market_index(&msg.channel)
        .with_context(|| format!("missing market index in channel {}", msg.channel))?;
    let instrument = instruments
        .get(&market_index)
        .context("unknown instrument")?;

    let mut ticks = Vec::with_capacity(msg.trades.len() + msg.liquidation_trades.len());
    for trade in msg.trades.iter().chain(msg.liquidation_trades.iter()) {
        ticks.push(parse_trade(trade, instrument, ts_init)?);
    }

    Ok(vec![NautilusWsMessage::Trades(ticks)])
}

fn parse_trade(
    trade: &LighterTrade,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> Result<TradeTick> {
    let price = parse_price(trade.price, instrument, "trade.price")?;
    let size = parse_quantity(trade.size, instrument, "trade.size")?;
    let aggressor = if trade.is_maker_ask {
        AggressorSide::Buyer
    } else {
        AggressorSide::Seller
    };
    let ts_event = parse_timestamp(trade.timestamp, ts_init);
    let trade_id = TradeId::new_checked(trade.trade_id.to_string())
        .context("invalid trade identifier in Lighter trade message")?;

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("failed to build TradeTick from Lighter trade")
}

fn parse_market_stats(
    msg: WsMarketStatsMessage,
    instruments: &HashMap<u32, InstrumentAny>,
    ts_init: UnixNanos,
) -> Result<Vec<NautilusWsMessage>> {
    let market_index = parse_market_index(&msg.channel)
        .with_context(|| format!("missing market index in channel {}", msg.channel))?;
    let instrument = instruments
        .get(&market_index)
        .context("unknown instrument")?;
    let instrument_id = instrument.id();

    let mut events = Vec::new();

    if let Some(mark_px) = msg.market_stats.mark_price {
        let price = parse_price(mark_px, instrument, "market_stats.mark_price")?;
        events.push(NautilusWsMessage::MarkPrice(MarkPriceUpdate::new(
            instrument_id,
            price,
            ts_init,
            ts_init,
        )));
    }

    if let Some(index_px) = msg.market_stats.index_price {
        let price = parse_price(index_px, instrument, "market_stats.index_price")?;
        events.push(NautilusWsMessage::IndexPrice(IndexPriceUpdate::new(
            instrument_id,
            price,
            ts_init,
            ts_init,
        )));
    }

    if let Some(rate) = msg.market_stats.funding_rate {
        let ts_event = msg
            .market_stats
            .funding_timestamp
            .map_or(ts_init, |ts| parse_timestamp(Some(ts), ts_init));
        events.push(NautilusWsMessage::FundingRate(FundingRateUpdate::new(
            instrument_id,
            rate,
            Some(ts_event),
            ts_init,
            ts_init,
        )));
    }

    Ok(events)
}

/// Extract market index from channel name (supports ":" or "/").
#[must_use]
pub fn parse_market_index(channel: &str) -> Option<u32> {
    channel
        .split([':', '/'])
        .next_back()
        .and_then(|s| u32::from_str(s).ok())
}

fn parse_timestamp(ts_ms: Option<i64>, ts_init: UnixNanos) -> UnixNanos {
    match ts_ms {
        Some(ms) if ms > 0 => {
            let nanos = (ms as u64).saturating_mul(1_000_000);
            UnixNanos::new(nanos)
        }
        _ => ts_init,
    }
}

fn parse_price(value: Decimal, instrument: &InstrumentAny, field: &str) -> Result<Price> {
    let f = value
        .to_f64()
        .with_context(|| format!("invalid price for {field}: {value}"))?;
    Ok(Price::new(f, instrument.price_precision()))
}

fn parse_quantity(value: Decimal, instrument: &InstrumentAny, field: &str) -> Result<Quantity> {
    let f = value
        .abs()
        .to_f64()
        .with_context(|| format!("invalid quantity for {field}: {value}"))?;
    Ok(Quantity::new(f, instrument.size_precision()))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use nautilus_core::time::get_atomic_clock_realtime;

    use crate::http::{
        models::OrderBooksResponse,
        parse::{instruments_from_defs, parse_instrument_defs},
    };

    fn orderbooks_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/test_data/lighter/http/orderbooks.json")
    }

    fn ws_fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../../tests/test_data/lighter/{name}"))
    }

    fn bootstrap_instrument() -> (HashMap<u32, InstrumentAny>, UnixNanos) {
        let data = std::fs::read_to_string(orderbooks_fixture()).unwrap();
        let resp: OrderBooksResponse = serde_json::from_str(&data).unwrap();
        let books = resp.into_books();
        let (defs, _) = parse_instrument_defs(&books).unwrap();
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let instruments = instruments_from_defs(&defs, ts_init).unwrap();
        let instrument = instruments[0].clone();

        let mut map = HashMap::new();
        map.insert(defs[0].market_index, instrument);
        (map, ts_init)
    }

    #[test]
    fn parses_order_book_snapshot() {
        let (instruments, ts_init) = bootstrap_instrument();
        let data = std::fs::read_to_string(ws_fixture("public_order_book_1.json")).unwrap();
        let raw: Vec<serde_json::Value> = serde_json::from_str(&data).unwrap();

        // Take the first snapshot message (skip connected)
        let snapshot_msg: WsMessage = serde_json::from_value(raw[1].clone()).unwrap();
        let events = parse_ws_message(snapshot_msg, &instruments, ts_init).unwrap();

        assert!(!events.is_empty());
        let deltas = match &events[0] {
            NautilusWsMessage::Deltas(d) => d,
            other => panic!("expected deltas, got {other:?}"),
        };

        assert!(
            deltas.deltas.len() > 10,
            "expected snapshot deltas to contain depth",
        );
        assert_eq!(deltas.deltas[0].sequence, 2760693);
    }

    #[test]
    fn parses_trade_snapshot() {
        let (instruments, ts_init) = bootstrap_instrument();
        let data = std::fs::read_to_string(ws_fixture("public_trade_1.json")).unwrap();
        let raw: Vec<serde_json::Value> = serde_json::from_str(&data).unwrap();

        let trades_msg: WsMessage = serde_json::from_value(raw[1].clone()).unwrap();
        let events = parse_ws_message(trades_msg, &instruments, ts_init).unwrap();

        let ticks = match &events[0] {
            NautilusWsMessage::Trades(t) => t,
            other => panic!("expected trades, got {other:?}"),
        };

        assert!(
            !ticks.is_empty(),
            "expected parsed trades from fixture snapshot",
        );
    }

    #[test]
    fn parses_market_stats() {
        let (instruments, ts_init) = bootstrap_instrument();
        let data = std::fs::read_to_string(ws_fixture("public_market_stats_1.json")).unwrap();
        let raw: Vec<serde_json::Value> = serde_json::from_str(&data).unwrap();

        let stats_msg: WsMessage = serde_json::from_value(raw[1].clone()).unwrap();
        let events = parse_ws_message(stats_msg, &instruments, ts_init).unwrap();

        assert!(
            events
                .iter()
                .any(|e| matches!(e, NautilusWsMessage::MarkPrice(_))),
            "expected mark price update",
        );
    }

    #[test]
    fn parse_market_index_supports_slash_and_colon() {
        assert_eq!(parse_market_index("order_book/42"), Some(42));
        assert_eq!(parse_market_index("trade:7"), Some(7));
        assert_eq!(parse_market_index("market_stats/0"), Some(0));
    }
}
