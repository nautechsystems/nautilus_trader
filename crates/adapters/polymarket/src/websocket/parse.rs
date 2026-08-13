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

use std::str::FromStr;

use aws_lc_rs::digest::{SHA1_FOR_LEGACY_USE_ONLY, digest};
use nautilus_core::{
    UnixNanos,
    correctness::{CorrectnessError, CorrectnessResult},
    datetime::NANOSECONDS_IN_MILLISECOND,
    hex,
};
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use serde::Serialize;

use super::messages::{
    PolymarketBookLevel, PolymarketBookSnapshot, PolymarketQuote, PolymarketTrade,
};
use crate::{
    common::{enums::PolymarketOrderSide, parse::determine_trade_id},
    http::parse::tick_relative_price_bounds,
};

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
    let value = Decimal::from_str(s).map_err(|e| CorrectnessError::PredicateViolation {
        message: format!("Invalid price '{s}': {e}"),
    })?;
    Price::from_decimal_dp(value, precision)
}

pub(crate) fn parse_quantity(s: &str, precision: u8) -> CorrectnessResult<Quantity> {
    let value = Decimal::from_str(s).map_err(|e| CorrectnessError::PredicateViolation {
        message: format!("Invalid quantity '{s}': {e}"),
    })?;
    Quantity::from_decimal_dp(value, precision)
}

pub(crate) fn verify_book_snapshot_hash(
    snap: &PolymarketBookSnapshot,
    min_order_size: Option<&str>,
    neg_risk: Option<bool>,
) -> anyhow::Result<bool> {
    let Some(expected) = snap.hash.as_deref() else {
        return Ok(false);
    };

    let Some(computed) = book_snapshot_hash(snap, min_order_size, neg_risk)? else {
        return Ok(false);
    };

    if computed != expected {
        anyhow::bail!(
            "Book snapshot hash mismatch for {}: expected {expected}, computed {computed}",
            snap.asset_id
        );
    }

    Ok(true)
}

fn book_snapshot_hash(
    snap: &PolymarketBookSnapshot,
    min_order_size: Option<&str>,
    neg_risk: Option<bool>,
) -> anyhow::Result<Option<String>> {
    let Some(min_order_size) = snap.min_order_size.as_deref().or(min_order_size) else {
        return Ok(None);
    };

    let Some(tick_size) = snap.tick_size.as_deref() else {
        return Ok(None);
    };

    let Some(neg_risk) = snap.neg_risk.or(neg_risk) else {
        return Ok(None);
    };

    let Some(last_trade_price) = snap.last_trade_price.as_deref() else {
        return Ok(None);
    };

    // Keep field order aligned with the server-compatible payload in the official SDK:
    // Polymarket/py-clob-client-v2@215fc63a8fd6ec3a10c7edb73997c9772d8686d3:utilities.py
    let preimage = BookSnapshotHashPreimage {
        market: snap.market.as_str(),
        asset_id: snap.asset_id.as_str(),
        timestamp: &snap.timestamp,
        hash: "",
        bids: &snap.bids,
        asks: &snap.asks,
        min_order_size,
        tick_size,
        neg_risk,
        last_trade_price,
    };

    let serialized = serde_json::to_vec(&preimage)?;
    let hash = digest(&SHA1_FOR_LEGACY_USE_ONLY, &serialized);

    Ok(Some(hex::encode(hash)))
}

#[derive(Serialize)]
struct BookSnapshotHashPreimage<'a> {
    market: &'a str,
    asset_id: &'a str,
    timestamp: &'a str,
    hash: &'static str,
    bids: &'a [PolymarketBookLevel],
    asks: &'a [PolymarketBookLevel],
    min_order_size: &'a str,
    tick_size: &'a str,
    neg_risk: bool,
    last_trade_price: &'a str,
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

/// Parses price change quotes into incremental book deltas.
///
/// Each result corresponds to one quote. The final successful delta carries
/// [`RecordFlag::F_LAST`], including when later quotes fail to parse.
pub fn parse_book_deltas(
    quotes: &[&PolymarketQuote],
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Vec<anyhow::Result<OrderBookDelta>> {
    let mut deltas = quotes
        .iter()
        .map(|change| {
            parse_book_delta(
                change,
                instrument_id,
                price_precision,
                size_precision,
                ts_event,
                ts_init,
            )
        })
        .collect::<Vec<_>>();

    if let Some(delta) = deltas
        .iter_mut()
        .rev()
        .find_map(|result| result.as_mut().ok())
    {
        delta.flags |= RecordFlag::F_LAST as u8;
    }

    deltas
}

fn parse_book_delta(
    change: &PolymarketQuote,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDelta> {
    let price = parse_price(&change.price, price_precision)?;
    let size = parse_quantity(&change.size, size_precision)?;
    let side = match change.side {
        PolymarketOrderSide::Buy => OrderSide::Buy,
        PolymarketOrderSide::Sell => OrderSide::Sell,
    };
    let (action, order_size) = if size.is_zero() {
        (BookAction::Delete, Quantity::zero(size_precision))
    } else {
        (BookAction::Update, size)
    };
    let order = BookOrder::new(side, price, order_size, 0);

    OrderBookDelta::new_checked(instrument_id, action, order, 0, 0, ts_event, ts_init)
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
/// Returns `None` if either side is empty and `drop_quotes_missing_side` is enabled.
pub fn parse_quote_from_snapshot(
    snap: &PolymarketBookSnapshot,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    price_increment: Price,
    drop_quotes_missing_side: bool,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<QuoteTick>> {
    if drop_quotes_missing_side && (snap.bids.is_empty() || snap.asks.is_empty()) {
        return Ok(None);
    }

    let ts_event = parse_timestamp_ms(&snap.timestamp)?;
    let (min_price, max_price) = tick_relative_price_bounds(price_increment.as_decimal())?;

    // Polymarket sends bids ascending and asks descending, so best-of-book is last.
    let (bid_price, bid_size) = match snap.bids.last() {
        Some(best_bid) => (
            parse_price(&best_bid.price, price_precision)?,
            parse_quantity(&best_bid.size, size_precision)?,
        ),
        None => (min_price, Quantity::zero(size_precision)),
    };
    let (ask_price, ask_size) = match snap.asks.last() {
        Some(best_ask) => (
            parse_price(&best_ask.price, price_precision)?,
            parse_quantity(&best_ask.size, size_precision)?,
        ),
        None => (max_price, Quantity::zero(size_precision)),
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

/// Parses a quote tick from a price change message using its best_bid/best_ask fields.
///
/// Returns `None` when either top-of-book side is absent or at the resolution
/// boundary and `drop_quotes_missing_side` is enabled.
/// Returns `None` for locked or crossed top-of-book prices.
/// When `last_quote` is provided the opposite side's size is carried forward
/// instead of being set to zero, matching the Python adapter's behavior.
#[expect(clippy::too_many_arguments)]
pub fn parse_quote_from_price_change(
    quote: &PolymarketQuote,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    price_increment: Price,
    drop_quotes_missing_side: bool,
    last_quote: Option<&QuoteTick>,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<QuoteTick>> {
    let bid_top = parse_bid_top(quote.best_bid.as_deref(), price_precision)?;
    let ask_top = parse_ask_top(quote.best_ask.as_deref(), price_precision)?;
    if drop_quotes_missing_side && (bid_top.is_none() || ask_top.is_none()) {
        return Ok(None);
    }

    let (min_price, max_price) = tick_relative_price_bounds(price_increment.as_decimal())?;
    let bid_missing = bid_top.is_none();
    let ask_missing = ask_top.is_none();
    let bid_price = match bid_top {
        Some(price) => price,
        None => min_price,
    };
    let ask_price = match ask_top {
        Some(price) => price,
        None => max_price,
    };

    if !bid_missing && !ask_missing && bid_price >= ask_price {
        return Ok(None);
    }

    let changed_price = parse_price(&quote.price, price_precision)?;

    let size = parse_quantity(&quote.size, size_precision)?;
    let zero = || Quantity::zero(size_precision);

    // Only use the changed level's size when it matches the best price,
    // otherwise preserve the previous quote's size for that side
    let (bid_size, ask_size) = match quote.side {
        PolymarketOrderSide::Buy => {
            let bid_size = if bid_missing {
                zero()
            } else if changed_price == bid_price {
                size
            } else {
                last_quote.map_or_else(zero, |q| q.bid_size)
            };
            let ask_size = if ask_missing {
                zero()
            } else {
                last_quote.map_or_else(zero, |q| q.ask_size)
            };
            (bid_size, ask_size)
        }
        PolymarketOrderSide::Sell => {
            let ask_size = if ask_missing {
                zero()
            } else if changed_price == ask_price {
                size
            } else {
                last_quote.map_or_else(zero, |q| q.ask_size)
            };
            let bid_size = if bid_missing {
                zero()
            } else {
                last_quote.map_or_else(zero, |q| q.bid_size)
            };
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

fn parse_bid_top(value: Option<&str>, precision: u8) -> CorrectnessResult<Option<Price>> {
    parse_top_price(value, precision, |value| value <= Decimal::ZERO)
}

fn parse_ask_top(value: Option<&str>, precision: u8) -> CorrectnessResult<Option<Price>> {
    parse_top_price(value, precision, |value| value >= Decimal::ONE)
}

fn parse_top_price(
    value: Option<&str>,
    precision: u8,
    is_missing: impl FnOnce(Decimal) -> bool,
) -> CorrectnessResult<Option<Price>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let decimal = Decimal::from_str(value).map_err(|e| CorrectnessError::PredicateViolation {
        message: format!("Invalid price '{value}': {e}"),
    })?;

    if is_missing(decimal) {
        return Ok(None);
    }

    Ok(Some(Price::from_decimal_dp(decimal, precision)?))
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::instruments::{Instrument, InstrumentAny};
    use rstest::rstest;

    use super::*;
    use crate::{
        http::parse::{
            create_instrument_from_def, parse_gamma_market, rebuild_instrument_with_tick_size,
        },
        websocket::messages::PolymarketQuotes,
    };

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

    fn test_instrument_with_tick(tick_size: &str) -> InstrumentAny {
        let instrument = test_instrument();
        let ts = UnixNanos::from(1_000_000_000u64);
        rebuild_instrument_with_tick_size(&instrument, tick_size, ts, ts).unwrap()
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
    fn test_book_snapshot_hash_matches_captured_snapshot() {
        let snap: PolymarketBookSnapshot = load("ws_book_snapshot_captured.json");

        assert_eq!(snap.min_order_size, None);
        assert_eq!(snap.neg_risk, None);
        assert_eq!(snap.tick_size.as_deref(), Some("0.01"));
        assert_eq!(snap.last_trade_price.as_deref(), Some("0.920"));
        assert_eq!(
            book_snapshot_hash(&snap, Some("5"), Some(false)).unwrap(),
            Some("ed47eb91f3c7985fac1cb18cb7c19535eddd3c0a".to_string())
        );
        assert!(verify_book_snapshot_hash(&snap, Some("5"), Some(false)).unwrap());
    }

    #[rstest]
    fn test_book_snapshot_hash_rejects_mismatch() {
        let mut snap: PolymarketBookSnapshot = load("ws_book_snapshot_captured.json");
        snap.bids[0].size = "3149725.71".to_string();

        let error = verify_book_snapshot_hash(&snap, Some("5"), Some(false)).unwrap_err();

        assert_eq!(
            error.to_string(),
            concat!(
                "Book snapshot hash mismatch for ",
                "350977769852917329387037893294763093471844346281449484439085576212613048126: ",
                "expected ed47eb91f3c7985fac1cb18cb7c19535eddd3c0a, ",
                "computed 6402b534c270a1ce46a75c62f1d7e3651182cc75"
            )
        );
    }

    #[rstest]
    fn test_book_snapshot_hash_allows_missing_hash() {
        let snap: PolymarketBookSnapshot = load("ws_book_snapshot_missing_hash.json");

        assert!(!verify_book_snapshot_hash(&snap, None, None).unwrap());
    }

    #[rstest]
    fn test_book_snapshot_hash_allows_incomplete_preimage() {
        let mut snap: PolymarketBookSnapshot = load("ws_book_snapshot_captured.json");
        snap.tick_size = None;
        snap.last_trade_price = None;

        assert!(!verify_book_snapshot_hash(&snap, Some("5"), Some(false)).unwrap());
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
        let ts_event = parse_timestamp_ms(&quotes.timestamp).unwrap();
        let ts_init = UnixNanos::from(1_000_000_000u64);
        let changes = quotes.price_changes.iter().collect::<Vec<_>>();

        let deltas = parse_book_deltas(
            &changes,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            ts_event,
            ts_init,
        )
        .into_iter()
        .collect::<anyhow::Result<Vec<_>>>()
        .unwrap();

        assert_eq!(deltas.len(), 2);

        // Exactly one delta carries F_LAST, and it must be the last one
        let f_last_count = deltas
            .iter()
            .filter(|d| d.flags & RecordFlag::F_LAST as u8 != 0)
            .count();
        assert_eq!(f_last_count, 1);
        assert_ne!(deltas.last().unwrap().flags & RecordFlag::F_LAST as u8, 0);
    }

    #[rstest]
    fn test_parse_book_deltas_zero_size_is_delete() {
        let mut quotes: PolymarketQuotes = load("ws_quotes.json");
        quotes.price_changes[0].size = "0".to_string();
        let instrument = test_instrument();
        let ts_event = parse_timestamp_ms(&quotes.timestamp).unwrap();
        let ts_init = UnixNanos::from(1_000_000_000u64);
        let changes = quotes.price_changes.iter().collect::<Vec<_>>();

        let deltas = parse_book_deltas(
            &changes,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            ts_event,
            ts_init,
        )
        .into_iter()
        .collect::<anyhow::Result<Vec<_>>>()
        .unwrap();

        assert_eq!(deltas[0].action, BookAction::Delete);
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
            instrument.price_increment(),
            true,
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
            instrument.price_increment(),
            true,
            ts_init,
        )
        .unwrap();

        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_quote_from_snapshot_empty_side_uses_boundary_when_drop_disabled() {
        let mut snap: PolymarketBookSnapshot = load("ws_book_snapshot.json");
        snap.asks.clear();
        let instrument = test_instrument_with_tick("0.005");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let quote = parse_quote_from_snapshot(
            &snap,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            false,
            ts_init,
        )
        .unwrap()
        .expect("quote should use boundary ask when drop is disabled");

        assert_eq!(quote.bid_price, Price::from("0.50"));
        assert_eq!(quote.bid_size, Quantity::from("200.00"));
        assert_eq!(quote.ask_price, Price::from("0.995"));
        assert_eq!(quote.ask_size, Quantity::from("0.00"));
    }

    #[rstest]
    fn test_parse_quote_from_snapshot_empty_bid_uses_boundary_when_drop_disabled() {
        let mut snap: PolymarketBookSnapshot = load("ws_book_snapshot.json");
        snap.bids.clear();
        let instrument = test_instrument_with_tick("0.0025");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let quote = parse_quote_from_snapshot(
            &snap,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            false,
            ts_init,
        )
        .unwrap()
        .expect("quote should use boundary bid when drop is disabled");

        assert_eq!(quote.bid_price, Price::from("0.0025"));
        assert_eq!(quote.bid_size, Quantity::from("0.00"));
        assert_eq!(quote.ask_price, Price::from("0.51"));
        assert_eq!(quote.ask_size, Quantity::from("150.00"));
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
            instrument.price_increment(),
            true,
            None,
            ts_event,
            ts_init,
        )
        .unwrap()
        .expect("quote should be Some when best_bid/best_ask present");

        assert_eq!(quote.instrument_id, instrument.id());
    }

    #[rstest]
    #[case(None)]
    #[case(Some("1"))]
    fn test_parse_quote_from_price_change_missing_side_drops_by_default(
        #[case] best_ask: Option<&str>,
    ) {
        let mut quotes: PolymarketQuotes = load("ws_quotes.json");
        quotes.price_changes[0].best_ask = best_ask.map(str::to_string);
        let instrument = test_instrument();
        let ts_event = parse_timestamp_ms(&quotes.timestamp).unwrap();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let result = parse_quote_from_price_change(
            &quotes.price_changes[0],
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            true,
            None,
            ts_event,
            ts_init,
        )
        .unwrap();

        assert!(result.is_none());
    }

    #[rstest]
    #[case(None)]
    #[case(Some("1"))]
    fn test_parse_quote_from_price_change_missing_side_uses_boundary_when_drop_disabled(
        #[case] best_ask: Option<&str>,
    ) {
        let mut quotes: PolymarketQuotes = load("ws_quotes.json");
        quotes.price_changes[0].best_ask = best_ask.map(str::to_string);
        let instrument = test_instrument_with_tick("0.005");
        let ts_event = parse_timestamp_ms(&quotes.timestamp).unwrap();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let quote = parse_quote_from_price_change(
            &quotes.price_changes[0],
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            false,
            None,
            ts_event,
            ts_init,
        )
        .unwrap()
        .expect("quote should use boundary ask when drop is disabled");

        assert_eq!(quote.bid_price, Price::from("0.51"));
        assert_eq!(quote.bid_size, Quantity::from("150.00"));
        assert_eq!(quote.ask_price, Price::from("0.995"));
        assert_eq!(quote.ask_size, Quantity::from("0.00"));
    }

    #[rstest]
    fn test_parse_quote_from_price_change_missing_bid_uses_boundary_when_drop_disabled() {
        let mut quotes: PolymarketQuotes = load("ws_quotes.json");
        quotes.price_changes[0].side = PolymarketOrderSide::Sell;
        quotes.price_changes[0].price = "0.52".to_string();
        quotes.price_changes[0].size = "75".to_string();
        quotes.price_changes[0].best_bid = Some("0".to_string());
        quotes.price_changes[0].best_ask = Some("0.52".to_string());
        let instrument = test_instrument_with_tick("0.0025");
        let ts_event = parse_timestamp_ms(&quotes.timestamp).unwrap();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let quote = parse_quote_from_price_change(
            &quotes.price_changes[0],
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            false,
            None,
            ts_event,
            ts_init,
        )
        .unwrap()
        .expect("quote should use boundary bid when drop is disabled");

        assert_eq!(quote.bid_price, Price::from("0.0025"));
        assert_eq!(quote.bid_size, Quantity::from("0.00"));
        assert_eq!(quote.ask_price, Price::from("0.52"));
        assert_eq!(quote.ask_size, Quantity::from("75.00"));
    }

    #[rstest]
    fn test_parse_quote_from_price_change_crossed_top_returns_none() {
        let mut quotes: PolymarketQuotes = load("ws_quotes.json");
        quotes.price_changes[0].best_bid = Some("0.70".to_string());
        quotes.price_changes[0].best_ask = Some("0.60".to_string());
        let instrument = test_instrument();
        let ts_event = parse_timestamp_ms(&quotes.timestamp).unwrap();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let result = parse_quote_from_price_change(
            &quotes.price_changes[0],
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            instrument.price_increment(),
            false,
            None,
            ts_event,
            ts_init,
        )
        .unwrap();

        assert!(result.is_none());
    }
}
