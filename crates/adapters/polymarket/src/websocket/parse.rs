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

//! Parse functions for converting Polymarket WebSocket messages to Nautilus data types.

use std::hash::{Hash, Hasher};

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::TradeId,
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use ustr::Ustr;

use super::messages::{PolymarketBookSnapshot, PolymarketQuote, PolymarketQuotes, PolymarketTrade};
use crate::common::enums::PolymarketOrderSide;

/// Parses a millisecond epoch timestamp string into [`UnixNanos`].
pub fn parse_timestamp_ms(ts: &str) -> anyhow::Result<UnixNanos> {
    let ms: u64 = ts
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid timestamp '{ts}': {e}"))?;
    let ns = ms
        .checked_mul(1_000_000)
        .ok_or_else(|| anyhow::anyhow!("Timestamp overflow for '{ts}'"))?;
    Ok(UnixNanos::from(ns))
}

fn parse_price(s: &str, precision: u8) -> anyhow::Result<Price> {
    let value: f64 = s
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid price '{s}': {e}"))?;
    Price::new_checked(value, precision)
}

fn parse_quantity(s: &str, precision: u8) -> anyhow::Result<Quantity> {
    let value: f64 = s
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid quantity '{s}': {e}"))?;
    Quantity::new_checked(value, precision)
}

/// Parses a book snapshot into [`OrderBookDeltas`] (CLEAR + ADD).
pub fn parse_book_snapshot(
    snap: &PolymarketBookSnapshot,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let ts_event = parse_timestamp_ms(&snap.timestamp)?;

    let bids_len = snap.bids.len();
    let asks_len = snap.asks.len();

    if bids_len == 0 && asks_len == 0 {
        anyhow::bail!("Empty book snapshot for {instrument_id}");
    }

    let total = bids_len + asks_len;
    let mut deltas = Vec::with_capacity(total + 1);

    deltas.push(OrderBookDelta::clear(instrument_id, 0, ts_event, ts_init));

    let mut count = 0;

    for level in &snap.bids {
        count += 1;
        let price = parse_price(&level.price, price_precision)?;
        let size = parse_quantity(&level.size, size_precision)?;
        let order = BookOrder::new(OrderSide::Buy, price, size, 0);
        let flags = if count == total {
            RecordFlag::F_LAST as u8 | RecordFlag::F_SNAPSHOT as u8
        } else {
            0
        };
        deltas.push(OrderBookDelta::new_checked(
            instrument_id,
            BookAction::Add,
            order,
            flags,
            0,
            ts_event,
            ts_init,
        )?);
    }

    for level in &snap.asks {
        count += 1;
        let price = parse_price(&level.price, price_precision)?;
        let size = parse_quantity(&level.size, size_precision)?;
        let order = BookOrder::new(OrderSide::Sell, price, size, 0);
        let flags = if count == total {
            RecordFlag::F_LAST as u8 | RecordFlag::F_SNAPSHOT as u8
        } else {
            0
        };
        deltas.push(OrderBookDelta::new_checked(
            instrument_id,
            BookAction::Add,
            order,
            flags,
            0,
            ts_event,
            ts_init,
        )?);
    }

    Ok(OrderBookDeltas::new(instrument_id, deltas))
}

/// Parses price change quotes into incremental [`OrderBookDeltas`].
pub fn parse_book_deltas(
    quotes: &PolymarketQuotes,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let ts_event = parse_timestamp_ms(&quotes.timestamp)?;

    let mut deltas = Vec::with_capacity(quotes.price_changes.len());

    for change in &quotes.price_changes {
        let price = parse_price(&change.price, price_precision)?;
        let size = parse_quantity(&change.size, size_precision)?;
        let side = match change.side {
            PolymarketOrderSide::Buy => OrderSide::Buy,
            PolymarketOrderSide::Sell => OrderSide::Sell,
        };

        let (action, order_size) = if size.is_zero() {
            (BookAction::Delete, Quantity::new(0.0, size_precision))
        } else {
            (BookAction::Update, size)
        };

        let order = BookOrder::new(side, price, order_size, 0);
        let flags = RecordFlag::F_LAST as u8;

        deltas.push(OrderBookDelta::new_checked(
            instrument_id,
            action,
            order,
            flags,
            0,
            ts_event,
            ts_init,
        )?);
    }

    Ok(OrderBookDeltas::new(instrument_id, deltas))
}

/// Parses a trade message into a [`TradeTick`].
pub fn parse_trade_tick(
    trade: &PolymarketTrade,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(&trade.price, instrument.price_precision())?;
    let size = parse_quantity(&trade.size, instrument.size_precision())?;
    let aggressor_side = match trade.side {
        PolymarketOrderSide::Buy => AggressorSide::Buyer,
        PolymarketOrderSide::Sell => AggressorSide::Seller,
    };
    let ts_event = parse_timestamp_ms(&trade.timestamp)?;

    // Deterministic trade ID from a hash of message fields (max 36 chars)
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    trade.asset_id.hash(&mut hasher);
    trade.price.hash(&mut hasher);
    trade.size.hash(&mut hasher);
    trade.timestamp.hash(&mut hasher);
    let trade_id = TradeId::new(Ustr::from(&format!("{:016x}", hasher.finish())));

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
}

/// Extracts a top-of-book [`QuoteTick`] from a book snapshot.
///
/// Returns `None` if either side is empty.
///
/// # Panics
///
/// Cannot panic: `.expect()` calls are guarded by the empty-side
/// early return above.
pub fn parse_quote_from_snapshot(
    snap: &PolymarketBookSnapshot,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<QuoteTick>> {
    if snap.bids.is_empty() || snap.asks.is_empty() {
        return Ok(None);
    }

    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let ts_event = parse_timestamp_ms(&snap.timestamp)?;

    // Polymarket sends bids ascending and asks descending, so best-of-book is last
    let best_bid = snap.bids.last().expect("bids not empty");
    let best_ask = snap.asks.last().expect("asks not empty");

    let bid_price = parse_price(&best_bid.price, price_precision)?;
    let ask_price = parse_price(&best_ask.price, price_precision)?;
    let bid_size = parse_quantity(&best_bid.size, size_precision)?;
    let ask_size = parse_quantity(&best_ask.size, size_precision)?;

    Ok(Some(QuoteTick::new_checked(
        instrument.id(),
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )?))
}

/// Parses a quote tick from a price change message using its best_bid/best_ask fields.
///
/// When `last_quote` is provided the opposite side's size is carried forward
/// instead of being set to zero, matching the Python adapter's behavior.
pub fn parse_quote_from_price_change(
    quote: &PolymarketQuote,
    instrument: &InstrumentAny,
    last_quote: Option<&QuoteTick>,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let bid_price = parse_price(&quote.best_bid, price_precision)?;
    let ask_price = parse_price(&quote.best_ask, price_precision)?;
    let changed_price = parse_price(&quote.price, price_precision)?;

    let size = parse_quantity(&quote.size, size_precision)?;
    let zero = || Quantity::new(0.0, size_precision);

    // Only use the changed level's size when it matches the best price,
    // otherwise preserve the previous quote's size for that side
    let (bid_size, ask_size) = match quote.side {
        PolymarketOrderSide::Buy => {
            let bid_size = if changed_price == bid_price {
                size
            } else {
                last_quote.map_or_else(zero, |q| q.bid_size)
            };
            let ask_size = last_quote.map_or_else(zero, |q| q.ask_size);
            (bid_size, ask_size)
        }
        PolymarketOrderSide::Sell => {
            let ask_size = if changed_price == ask_price {
                size
            } else {
                last_quote.map_or_else(zero, |q| q.ask_size)
            };
            let bid_size = last_quote.map_or_else(zero, |q| q.bid_size);
            (bid_size, ask_size)
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
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;

    use super::*;
    use crate::http::parse::{create_instrument_from_def, parse_gamma_market};

    fn load<T: serde::de::DeserializeOwned>(filename: &str) -> T {
        let content =
            std::fs::read_to_string(format!("test_data/{filename}")).expect("test data missing");
        serde_json::from_str(&content).expect("parse failed")
    }

    fn test_instrument() -> InstrumentAny {
        let market: crate::http::models::GammaMarket = load("gamma_market.json");
        let defs = parse_gamma_market(&market).unwrap();
        create_instrument_from_def(&defs[0], UnixNanos::from(1_000_000_000u64)).unwrap()
    }

    #[rstest]
    fn test_parse_timestamp_ms() {
        let ns = parse_timestamp_ms("1703875200000").unwrap();
        assert_eq!(ns, UnixNanos::from(1_703_875_200_000_000_000u64));
    }

    #[rstest]
    fn test_parse_timestamp_ms_invalid() {
        assert!(parse_timestamp_ms("not_a_number").is_err());
    }

    #[rstest]
    fn test_parse_book_snapshot() {
        let snap: PolymarketBookSnapshot = load("ws_book_snapshot.json");
        let instrument = test_instrument();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let deltas = parse_book_snapshot(&snap, &instrument, ts_init).unwrap();

        // CLEAR + 3 bids + 3 asks = 7 deltas
        assert_eq!(deltas.deltas.len(), 7);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[1].action, BookAction::Add);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
        assert_eq!(deltas.deltas[4].action, BookAction::Add);
        assert_eq!(deltas.deltas[4].order.side, OrderSide::Sell);

        // Last delta should have F_LAST | F_SNAPSHOT flags
        let last = deltas.deltas.last().unwrap();
        assert_ne!(last.flags & RecordFlag::F_LAST as u8, 0);
        assert_ne!(last.flags & RecordFlag::F_SNAPSHOT as u8, 0);
    }

    #[rstest]
    fn test_parse_book_deltas() {
        let quotes: PolymarketQuotes = load("ws_quotes.json");
        let instrument = test_instrument();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let deltas = parse_book_deltas(&quotes, &instrument, ts_init).unwrap();

        assert_eq!(deltas.deltas.len(), 2);

        for delta in &deltas.deltas {
            assert_ne!(delta.flags & RecordFlag::F_LAST as u8, 0);
        }
    }

    #[rstest]
    fn test_parse_book_deltas_zero_size_is_delete() {
        let mut quotes: PolymarketQuotes = load("ws_quotes.json");
        quotes.price_changes[0].size = "0".to_string();
        let instrument = test_instrument();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let deltas = parse_book_deltas(&quotes, &instrument, ts_init).unwrap();

        assert_eq!(deltas.deltas[0].action, BookAction::Delete);
    }

    #[rstest]
    fn test_parse_trade_tick() {
        let trade: PolymarketTrade = load("ws_last_trade.json");
        let instrument = test_instrument();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let tick = parse_trade_tick(&trade, &instrument, ts_init).unwrap();

        assert_eq!(tick.instrument_id, instrument.id());
        assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
        assert_eq!(tick.ts_event, UnixNanos::from(1_703_875_202_000_000_000u64));
    }

    #[rstest]
    fn test_parse_trade_tick_deterministic_id() {
        let trade: PolymarketTrade = load("ws_last_trade.json");
        let instrument = test_instrument();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let tick1 = parse_trade_tick(&trade, &instrument, ts_init).unwrap();
        let tick2 = parse_trade_tick(&trade, &instrument, ts_init).unwrap();

        assert_eq!(tick1.trade_id, tick2.trade_id);
    }

    #[rstest]
    fn test_parse_quote_from_snapshot() {
        let snap: PolymarketBookSnapshot = load("ws_book_snapshot.json");
        let instrument = test_instrument();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let quote = parse_quote_from_snapshot(&snap, &instrument, ts_init)
            .unwrap()
            .unwrap();

        assert_eq!(quote.instrument_id, instrument.id());
        assert_eq!(quote.bid_price, Price::from("0.50"));
        assert_eq!(quote.ask_price, Price::from("0.51"));
        assert_eq!(
            quote.ts_event,
            UnixNanos::from(1_703_875_200_000_000_000u64)
        );
    }

    #[rstest]
    fn test_parse_quote_from_snapshot_empty_side_returns_none() {
        let mut snap: PolymarketBookSnapshot = load("ws_book_snapshot.json");
        snap.bids.clear();
        let instrument = test_instrument();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let result = parse_quote_from_snapshot(&snap, &instrument, ts_init).unwrap();

        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_quote_from_price_change() {
        let quotes: PolymarketQuotes = load("ws_quotes.json");
        let instrument = test_instrument();
        let ts_event = parse_timestamp_ms(&quotes.timestamp).unwrap();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let quote = parse_quote_from_price_change(
            &quotes.price_changes[0],
            &instrument,
            None,
            ts_event,
            ts_init,
        )
        .unwrap();

        assert_eq!(quote.instrument_id, instrument.id());
    }
}
