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

//! Shared utilities for OKX criterion benches.
//!
//! Fixtures are real venue captures from `test_data/` loaded via `include_str!`
//! so bench inputs track the parser test corpus; canonical bench numbers thus
//! describe the same wire shapes used to verify parser correctness.
//!
//! Each criterion bench is a separate compilation unit that pulls in this
//! module, but uses only a subset of the helpers and fixtures. Without the
//! module-level `allow`, the unused subset in any given bench triggers
//! per-crate dead-code warnings.

#![allow(dead_code)]

use ahash::AHashMap;
use nautilus_common::messages::ExecutionEvent;
use nautilus_core::{AtomicTime, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::AccountType,
    identifiers::{AccountId, InstrumentId, Symbol, TraderId},
    instruments::{CryptoPerpetual, CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use nautilus_okx::common::consts::OKX_VENUE;
use ustr::Ustr;

pub(crate) const TRADER_ID: &str = "BENCH-001";
pub(crate) const ACCOUNT_ID: &str = "OKX-001";

#[must_use]
pub(crate) fn clock() -> &'static AtomicTime {
    get_atomic_clock_realtime()
}

#[must_use]
pub(crate) fn trader_id() -> TraderId {
    TraderId::from(TRADER_ID)
}

#[must_use]
pub(crate) fn account_id() -> AccountId {
    AccountId::from(ACCOUNT_ID)
}

/// BTC-USDT spot pair. Matches the `instId` in `ws_books_*.json`, `ws_bbo_tbt.json`,
/// `ws_candle.json`, and `ws_funding_rate.json` so the same instrument cache
/// serves every inbound bench.
#[must_use]
pub(crate) fn btc_usdt_spot() -> InstrumentAny {
    spot_instrument("BTC", "USDT", 2, 8)
}

/// BTC-USD spot pair (used by `ws_trades.json`).
#[must_use]
pub(crate) fn btc_usd_spot() -> InstrumentAny {
    spot_instrument("BTC", "USD", 1, 8)
}

/// BTC-USDT-SWAP perpetual (used by `ws_funding_rate.json` and `ws_orders.json`).
#[must_use]
pub(crate) fn btc_usdt_swap() -> InstrumentAny {
    perp_instrument("BTC-USDT-SWAP", "BTC", "USDT", 1, 2)
}

fn spot_instrument(
    base: &str,
    quote: &str,
    price_precision: u8,
    size_precision: u8,
) -> InstrumentAny {
    let symbol_str = format!("{base}-{quote}");
    let raw_symbol = Symbol::new(&symbol_str);
    let instrument_id = InstrumentId::new(raw_symbol, *OKX_VENUE);
    let price_increment = Price::new(10f64.powi(-(price_precision as i32)), price_precision);
    let size_increment = Quantity::new(10f64.powi(-(size_precision as i32)), size_precision);
    InstrumentAny::CurrencyPair(CurrencyPair::new(
        instrument_id,
        raw_symbol,
        Currency::from(base),
        Currency::from(quote),
        price_precision,
        size_precision,
        price_increment,
        size_increment,
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
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    ))
}

fn perp_instrument(
    symbol: &str,
    base: &str,
    quote: &str,
    price_precision: u8,
    size_precision: u8,
) -> InstrumentAny {
    let raw_symbol = Symbol::new(symbol);
    let instrument_id = InstrumentId::new(raw_symbol, *OKX_VENUE);
    let price_increment = Price::new(10f64.powi(-(price_precision as i32)), price_precision);
    let size_increment = Quantity::new(10f64.powi(-(size_precision as i32)), size_precision);
    InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        Currency::from(base),
        Currency::from(quote),
        Currency::from(quote),
        false,
        price_precision,
        size_precision,
        price_increment,
        size_increment,
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
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    ))
}

/// Cache keyed by `instId` (`Ustr`) - the lookup shape every OKX parse function uses.
#[must_use]
pub(crate) fn instrument_cache() -> AHashMap<Ustr, InstrumentAny> {
    let mut cache = AHashMap::new();
    let btc_usdt = btc_usdt_spot();
    let btc_usd = btc_usd_spot();
    let btc_swap = btc_usdt_swap();
    cache.insert(Ustr::from("BTC-USDT"), btc_usdt);
    cache.insert(Ustr::from("BTC-USD"), btc_usd);
    cache.insert(Ustr::from("BTC-USDT-SWAP"), btc_swap);
    cache
}

/// Builds an [`ExecutionEventEmitter`] connected to an unbounded channel whose
/// receiver is returned alongside the emitter; benches must keep the receiver
/// alive (drop closes the channel and turns `send_order_event` into a
/// warn-logging no-op which skews the measurement).
#[must_use]
pub(crate) fn bench_emitter() -> (
    ExecutionEventEmitter,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) {
    let mut emitter = ExecutionEventEmitter::new(
        clock(),
        trader_id(),
        account_id(),
        AccountType::Cash,
        Some(Currency::from("USDT")),
    );
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    emitter.set_sender(tx);
    (emitter, rx)
}

pub(crate) mod fixtures {
    //! Real venue captures from `test_data/`, kept in sync with parser tests.

    pub(crate) const TRADE: &str = include_str!("../../test_data/ws_trades.json");
    pub(crate) const BOOK_SNAPSHOT: &str = include_str!("../../test_data/ws_books_snapshot.json");
    pub(crate) const BOOK_UPDATE: &str = include_str!("../../test_data/ws_books_update.json");
    pub(crate) const BBO_TBT: &str = include_str!("../../test_data/ws_bbo_tbt.json");
    pub(crate) const CANDLE: &str = include_str!("../../test_data/ws_candle.json");
    pub(crate) const FUNDING_RATE: &str = include_str!("../../test_data/ws_funding_rate.json");
    pub(crate) const TICKERS: &str = include_str!("../../test_data/ws_tickers.json");
    pub(crate) const ORDERS: &str = include_str!("../../test_data/ws_orders.json");
    pub(crate) const ACCOUNT: &str = include_str!("../../test_data/ws_account.json");

    /// Single-record mark price frame matching the OKX `mark-price` channel wire shape.
    /// Constructed inline because there is no `ws_mark_price.json` capture and the
    /// shape is well-specified.
    pub(crate) const MARK_PRICE: &str = r#"{
        "arg": {"channel": "mark-price", "instId": "BTC-USDT"},
        "data": [{
            "instType": "MARGIN",
            "instId": "BTC-USDT",
            "markPx": "42219.9",
            "ts": "1597026383085"
        }]
    }"#;

    /// Synthetic accepted-order frame (state=live, no fill fields populated) so
    /// `parse_order_msg` exercises the `OrderStatusReport` branch. The captured
    /// `ws_orders.json` is a filled-state record and only exercises the fill
    /// branch, so this complement is constructed inline.
    pub(crate) const ORDER_LIVE: &str = r#"{
        "arg": {"channel": "orders", "instType": "SWAP"},
        "data": [{
            "accFillSz": "0",
            "algoClOrdId": "",
            "algoId": "",
            "attachAlgoClOrdId": "",
            "attachAlgoOrds": [],
            "avgPx": "",
            "cTime": "1746947317401",
            "cancelSource": "",
            "cancelSourceReason": "",
            "category": "normal",
            "ccy": "USDT",
            "clOrdId": "001BTCUSDT20250106002",
            "execType": "",
            "fee": "0",
            "feeCcy": "USDT",
            "fillPx": "",
            "fillSz": "0",
            "fillTime": "0",
            "instId": "BTC-USDT-SWAP",
            "instType": "SWAP",
            "isTpLimit": "false",
            "lever": "2.0",
            "linkedAlgoOrd": {"algoId": ""},
            "ordId": "2497956918703120385",
            "ordType": "limit",
            "pnl": "0",
            "posSide": "long",
            "px": "100000.0",
            "pxType": "",
            "pxUsd": "",
            "pxVol": "",
            "quickMgnType": "",
            "rebate": "0",
            "rebateCcy": "USDT",
            "reduceOnly": "false",
            "side": "buy",
            "slOrdPx": "",
            "slTriggerPx": "",
            "slTriggerPxType": "",
            "source": "",
            "state": "live",
            "stpId": "",
            "stpMode": "cancel_maker",
            "sz": "0.03",
            "tag": "",
            "tdMode": "isolated",
            "tgtCcy": "",
            "tpOrdPx": "",
            "tpTriggerPx": "",
            "tpTriggerPxType": "",
            "tradeId": "",
            "uTime": "1746947317401"
        }]
    }"#;

    /// Single-record index price frame matching the OKX `index-tickers` channel shape.
    pub(crate) const INDEX_PRICE: &str = r#"{
        "arg": {"channel": "index-tickers", "instId": "BTC-USDT"},
        "data": [{
            "instId": "BTC-USDT",
            "idxPx": "42220.1",
            "high24h": "42500.0",
            "low24h": "42000.0",
            "open24h": "42100.0",
            "sodUtc0": "42150.0",
            "sodUtc8": "42180.0",
            "ts": "1597026383085"
        }]
    }"#;
}
