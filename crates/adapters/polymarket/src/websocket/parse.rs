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

use nautilus_core::{
    UnixNanos,
    correctness::{CorrectnessError, CorrectnessResult},
    datetime::NANOSECONDS_IN_MILLISECOND,
};
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

use super::messages::{PolymarketBookSnapshot, PolymarketQuote, PolymarketQuotes, PolymarketTrade};
use crate::common::{enums::PolymarketOrderSide, parse::determine_trade_id};

/// Parses a millisecond epoch timestamp string into [`UnixNanos`].
pub fn parse_timestamp_ms(ts: &str) -> anyhow::Result<UnixNanos> {
    let ms: u64 = ts
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid timestamp '{ts}': {e}"))?;
    let ns = ms
        .checked_mul(NANOSECONDS_IN_MILLISECOND)
        .ok_or_else(|| anyhow::anyhow!("Timestamp overflow for '{ts}'"))?;
    Ok(UnixNanos::from(ns))
}

pub(crate) fn parse_price(s: &str, precision: u8) -> CorrectnessResult<Price> {
    let value: f64 = s
        .parse()
        .map_err(|e| CorrectnessError::PredicateViolation {
            message: format!("Invalid price '{s}': {e}"),
        })?;
    Price::new_checked(value, precision)
}

pub(crate) fn parse_quantity(s: &str, precision: u8) -> CorrectnessResult<Quantity> {
    let value: f64 = s
        .parse()
        .map_err(|e| CorrectnessError::PredicateViolation {
            message: format!("Invalid quantity '{s}': {e}"),
        })?;
    Quantity::new_checked(value, precision)
}

/// Parses a book snapshot into [`OrderBookDeltas`] (CLEAR + ADD).
pub fn parse_book_snapshot(
    snap: &PolymarketBookSnapshot,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let ts_event = parse_timestamp_ms(&snap.timestamp)?;

    let bids_len = snap.bids.len();
    let asks_len = snap.asks.len();

    if bids_len == 0 && asks_len == 0 {
        anyhow::bail!("Empty book snapshot for {instrument_id}");
    }

    let total = bids_len + asks_len;
    let mut deltas = Vec::with_capacity(total + 1);

    // Every snapshot delta (including the opening CLEAR) carries F_SNAPSHOT so
    // downstream consumers can recognise the rebuild; F_LAST closes the batch
    // on the final delta. `OrderBookDelta::clear` already sets F_SNAPSHOT.
    let snapshot_flag = RecordFlag::F_SNAPSHOT as u8;
    deltas.push(OrderBookDelta::clear(instrument_id, 0, ts_event, ts_init));

    let mut count = 0;

    for level in &snap.bids {
        count += 1;
        let price = parse_price(&level.price, price_precision)?;
        let size = parse_quantity(&level.size, size_precision)?;
        let order = BookOrder::new(OrderSide::Buy, price, size, 0);

        let mut flags = snapshot_flag;
        if count == total {
            flags |= RecordFlag::F_LAST as u8;
        }

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

        let mut flags = snapshot_flag;
        if count == total {
            flags |= RecordFlag::F_LAST as u8;
        }

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
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let ts_event = parse_timestamp_ms(&quotes.timestamp)?;

    let total = quotes.price_changes.len();
    let mut deltas = Vec::with_capacity(total);

    for (idx, change) in quotes.price_changes.iter().enumerate() {
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
        let flags = if idx == total - 1 {
            RecordFlag::F_LAST as u8
        } else {
            0
        };

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
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(&trade.price, price_precision)?;
    let size = parse_quantity(&trade.size, size_precision)?;
    let aggressor_side = match trade.side {
        PolymarketOrderSide::Buy => AggressorSide::Buyer,
        PolymarketOrderSide::Sell => AggressorSide::Seller,
    };
    let ts_event = parse_timestamp_ms(&trade.timestamp)?;

    let trade_id = determine_trade_id(
        &trade.asset_id,
        trade.side,
        &trade.price,
        &trade.size,
        &trade.timestamp,
    );

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
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<QuoteTick>> {
    if snap.bids.is_empty() || snap.asks.is_empty() {
        return Ok(None);
    }

    let ts_event = parse_timestamp_ms(&snap.timestamp)?;

    // Polymarket sends bids ascending and asks descending, so best-of-book is last
    let best_bid = snap.bids.last().expect("bids not empty");
    let best_ask = snap.asks.last().expect("asks not empty");

    let bid_price = parse_price(&best_bid.price, price_precision)?;
    let ask_price = parse_price(&best_ask.price, price_precision)?;
    let bid_size = parse_quantity(&best_bid.size, size_precision)?;
    let ask_size = parse_quantity(&best_ask.size, size_precision)?;

    Ok(Some(QuoteTick::new_checked(
        instrument_id,
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
/// Returns `None` when either best_bid or best_ask is absent (empty book side).
/// When `last_quote` is provided the opposite side's size is carried forward
/// instead of being set to zero, matching the Python adapter's behavior.
pub fn parse_quote_from_price_change(
    quote: &PolymarketQuote,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    last_quote: Option<&QuoteTick>,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<QuoteTick>> {
    let (Some(best_bid), Some(best_ask)) = (&quote.best_bid, &quote.best_ask) else {
        return Ok(None);
    };
    let bid_price = parse_price(best_bid, price_precision)?;
    let ask_price = parse_price(best_ask, price_precision)?;
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

    Ok(Some(QuoteTick::new_checked(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )?))
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::instruments::{Instrument, InstrumentAny};
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

        let deltas = parse_book_snapshot(
            &snap,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            ts_init,
        )
        .unwrap();

        // CLEAR + 3 bids + 3 asks = 7 deltas
        assert_eq!(deltas.deltas.len(), 7);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[1].action, BookAction::Add);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
        assert_eq!(deltas.deltas[4].action, BookAction::Add);
        assert_eq!(deltas.deltas[4].order.side, OrderSide::Sell);

        // Every snapshot delta carries F_SNAPSHOT
        for delta in &deltas.deltas {
            assert_ne!(delta.flags & RecordFlag::F_SNAPSHOT as u8, 0);
        }

        // Exactly one delta carries F_LAST, and it must be the last one
        let f_last_count = deltas
            .deltas
            .iter()
            .filter(|d| d.flags & RecordFlag::F_LAST as u8 != 0)
            .count();
        assert_eq!(f_last_count, 1);
        assert_ne!(
            deltas.deltas.last().unwrap().flags & RecordFlag::F_LAST as u8,
            0
        );
    }

    #[rstest]
    fn test_parse_book_deltas() {
        let quotes: PolymarketQuotes = load("ws_quotes.json");
        let instrument = test_instrument();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let deltas = parse_book_deltas(
            &quotes,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            ts_init,
        )
        .unwrap();

        assert_eq!(deltas.deltas.len(), 2);

        // Exactly one delta carries F_LAST, and it must be the last one
        let f_last_count = deltas
            .deltas
            .iter()
            .filter(|d| d.flags & RecordFlag::F_LAST as u8 != 0)
            .count();
        assert_eq!(f_last_count, 1);
        assert_ne!(
            deltas.deltas.last().unwrap().flags & RecordFlag::F_LAST as u8,
            0
        );
    }

    #[rstest]
    fn test_parse_book_deltas_zero_size_is_delete() {
        let mut quotes: PolymarketQuotes = load("ws_quotes.json");
        quotes.price_changes[0].size = "0".to_string();
        let instrument = test_instrument();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let deltas = parse_book_deltas(
            &quotes,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            ts_init,
        )
        .unwrap();

        assert_eq!(deltas.deltas[0].action, BookAction::Delete);
    }

    #[rstest]
    fn test_parse_trade_tick() {
        let trade: PolymarketTrade = load("ws_last_trade.json");
        let instrument = test_instrument();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let tick = parse_trade_tick(
            &trade,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            ts_init,
        )
        .unwrap();

        assert_eq!(tick.instrument_id, instrument.id());
        assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
        assert_eq!(tick.ts_event, UnixNanos::from(1_703_875_202_000_000_000u64));
    }

    #[rstest]
    fn test_parse_trade_tick_deterministic_id() {
        let trade: PolymarketTrade = load("ws_last_trade.json");
        let instrument = test_instrument();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let tick1 = parse_trade_tick(
            &trade,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            ts_init,
        )
        .unwrap();
        let tick2 = parse_trade_tick(
            &trade,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            ts_init,
        )
        .unwrap();

        assert_eq!(tick1.trade_id, tick2.trade_id);
    }

    #[rstest]
    fn test_parse_quote_from_snapshot() {
        let snap: PolymarketBookSnapshot = load("ws_book_snapshot.json");
        let instrument = test_instrument();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let quote = parse_quote_from_snapshot(
            &snap,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            ts_init,
        )
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

        let result = parse_quote_from_snapshot(
            &snap,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            ts_init,
        )
        .unwrap();

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
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            None,
            ts_event,
            ts_init,
        )
        .unwrap()
        .expect("quote should be Some when best_bid/best_ask present");

        assert_eq!(quote.instrument_id, instrument.id());
    }
}
