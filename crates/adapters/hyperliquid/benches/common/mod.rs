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

//! Shared utilities for hyperliquid criterion benches.
//!
//! Fixtures live as inline `&'static str` consts to keep benches deterministic
//! and self-contained; real venue captures in `test_data/` are reserved for
//! parser correctness tests.
//!
//! Each criterion bench is a separate compilation unit that pulls in this
//! module, but uses only a subset of the helpers and fixtures. Without the
//! module-level `allow`, the unused subset in any given bench triggers
//! per-crate dead-code warnings.

#![allow(dead_code)]

use ahash::AHashMap;
use nautilus_common::{cache::Cache, messages::ExecutionEvent};
use nautilus_core::{AtomicTime, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_hyperliquid::common::consts::HYPERLIQUID_VENUE;
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::AccountType,
    identifiers::{AccountId, InstrumentId, Symbol, TraderId},
    instruments::{CryptoPerpetual, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use ustr::Ustr;

pub(crate) const TRADER_ID: &str = "BENCH-001";
pub(crate) const ACCOUNT_ID: &str = "HYPERLIQUID-001";

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

#[must_use]
pub(crate) fn btc_perp() -> InstrumentAny {
    perp_instrument("BTC", 2, 4)
}

#[must_use]
pub(crate) fn eth_perp() -> InstrumentAny {
    perp_instrument("ETH", 2, 4)
}

fn perp_instrument(coin: &str, price_precision: u8, size_precision: u8) -> InstrumentAny {
    let symbol_str = format!("{coin}-USD-PERP");
    let raw_symbol = Symbol::new(coin);
    let instrument_id = InstrumentId::new(Symbol::new(&symbol_str), *HYPERLIQUID_VENUE);
    let price_increment = Price::new(10f64.powi(-(price_precision as i32)), price_precision);
    let size_increment = Quantity::new(10f64.powi(-(size_precision as i32)), size_precision);
    InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        Currency::from(coin),
        Currency::from("USDC"),
        Currency::from("USDC"),
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

#[must_use]
pub(crate) fn instrument_cache() -> AHashMap<Ustr, InstrumentAny> {
    let mut cache = AHashMap::new();
    let btc = btc_perp();
    let eth = eth_perp();
    cache.insert(Ustr::from("BTC"), btc);
    cache.insert(Ustr::from("ETH"), eth);
    cache
}

/// Builds an [`ExecutionEventEmitter`] connected to an unbounded channel whose
/// receiver is returned alongside the emitter; tests/benches must keep the
/// receiver alive (drop closes the channel and turns `send_order_event` into a
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
        AccountType::Margin,
        Some(Currency::from("USDC")),
    );
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    emitter.set_sender(tx);
    (emitter, rx)
}

/// Convenience cache (used by future dispatch benches that need it).
#[must_use]
pub(crate) fn empty_cache() -> Cache {
    Cache::default()
}

pub(crate) mod fixtures {
    //! Inline WS frame strings shaped exactly like the venue wire format. Each
    //! fixture exercises one [`HyperliquidWsMessage`] variant end-to-end and is
    //! kept small enough to be obvious at a glance.

    pub(crate) const TRADE: &str = r#"{
        "channel": "trades",
        "data": [{
            "coin": "BTC",
            "side": "B",
            "px": "98450.5",
            "sz": "0.0123",
            "hash": "0xabc123",
            "time": 1733833200000,
            "tid": 987654321,
            "users": ["0x1111111111111111111111111111111111111111", "0x2222222222222222222222222222222222222222"]
        }]
    }"#;

    pub(crate) const BOOK_L2: &str = r#"{
        "channel": "l2Book",
        "data": {
            "coin": "BTC",
            "levels": [
                [
                    {"px": "98450.5", "sz": "2.5", "n": 3},
                    {"px": "98449.0", "sz": "1.8", "n": 2},
                    {"px": "98448.0", "sz": "0.75", "n": 1},
                    {"px": "98447.0", "sz": "3.2", "n": 4},
                    {"px": "98446.0", "sz": "1.1", "n": 2},
                    {"px": "98445.0", "sz": "2.0", "n": 3},
                    {"px": "98444.0", "sz": "0.5", "n": 1},
                    {"px": "98443.0", "sz": "1.4", "n": 2},
                    {"px": "98442.0", "sz": "0.9", "n": 1},
                    {"px": "98441.0", "sz": "1.7", "n": 2}
                ],
                [
                    {"px": "98451.0", "sz": "1.5", "n": 2},
                    {"px": "98452.0", "sz": "2.1", "n": 3},
                    {"px": "98453.0", "sz": "0.9", "n": 1},
                    {"px": "98454.0", "sz": "1.7", "n": 2},
                    {"px": "98455.0", "sz": "2.3", "n": 4},
                    {"px": "98456.0", "sz": "0.6", "n": 1},
                    {"px": "98457.0", "sz": "1.2", "n": 2},
                    {"px": "98458.0", "sz": "0.8", "n": 1},
                    {"px": "98459.0", "sz": "1.5", "n": 2},
                    {"px": "98460.0", "sz": "2.0", "n": 3}
                ]
            ],
            "time": 1733833200000
        }
    }"#;

    pub(crate) const BBO: &str = r#"{
        "channel": "bbo",
        "data": {
            "coin": "BTC",
            "time": 1733833200000,
            "bbo": [
                {"px": "98450.5", "sz": "2.5", "n": 3},
                {"px": "98451.0", "sz": "1.5", "n": 2}
            ]
        }
    }"#;

    pub(crate) const CANDLE: &str = r#"{
        "channel": "candle",
        "data": {
            "t": 1733833200000,
            "T": 1733833260000,
            "s": "BTC",
            "i": "1m",
            "o": "98450.0",
            "c": "98460.0",
            "h": "98470.0",
            "l": "98440.0",
            "v": "10.5",
            "n": 42
        }
    }"#;

    pub(crate) const ALL_MIDS: &str = r#"{
        "channel": "allMids",
        "data": {
            "mids": {
                "BTC": "98455.5",
                "ETH": "2114.25",
                "SOL": "94.88"
            }
        }
    }"#;

    pub(crate) const ACTIVE_ASSET_CTX_PERP: &str = r#"{
        "channel": "activeAssetCtx",
        "data": {
            "coin": "BTC",
            "ctx": {
                "dayNtlVlm": "1000000.0",
                "prevDayPx": "97000.0",
                "markPx": "98455.5",
                "midPx": "98455.0",
                "impactPxs": ["98454.0", "98456.0"],
                "dayBaseVlm": "100.0",
                "funding": "0.0001",
                "openInterest": "1500.0",
                "oraclePx": "98460.0",
                "premium": "-0.0001"
            }
        }
    }"#;

    pub(crate) const ORDER_UPDATE: &str = r#"{
        "channel": "orderUpdates",
        "data": [{
            "order": {
                "coin": "BTC",
                "side": "B",
                "limitPx": "98000.0",
                "sz": "0.5",
                "oid": 430481837807,
                "timestamp": 1733833200000,
                "origSz": "1.0",
                "cloid": "0xd211f1c27288259290850338d22132a0"
            },
            "status": "open",
            "statusTimestamp": 1733833200000
        }]
    }"#;

    pub(crate) const USER_FILL: &str = r#"{
        "channel": "user",
        "data": {
            "fills": [{
                "coin": "BTC",
                "px": "98450.5",
                "sz": "0.1",
                "side": "B",
                "time": 1733833200000,
                "startPosition": "0.0",
                "dir": "Open Long",
                "closedPnl": "0.0",
                "hash": "0xabc123",
                "oid": 430481837807,
                "crossed": true,
                "fee": "0.05",
                "tid": 98765,
                "feeToken": "USDC",
                "cloid": "0xd211f1c27288259290850338d22132a0"
            }]
        }
    }"#;
}
