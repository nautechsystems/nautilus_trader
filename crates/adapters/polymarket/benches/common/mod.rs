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

//! Shared utilities for polymarket criterion benches.
//!
//! Fixtures live as inline `&'static str` consts shaped exactly like the venue
//! wire format. Keeping them inline (rather than reading from `test_data/`)
//! makes each bench self-contained and removes filesystem variance.
//!
//! Each criterion bench is a separate compilation unit that pulls in this
//! module, but uses only a subset of the helpers and fixtures. Without the
//! module-level `allow`, the unused subset in any given bench triggers
//! per-crate dead-code warnings.

#![allow(dead_code)]

use ahash::AHashMap;
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::AssetClass,
    identifiers::{AccountId, InstrumentId, Symbol},
    instruments::{BinaryOption, Instrument, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use nautilus_polymarket::common::{consts::POLYMARKET_VENUE, credential::Credential};
use ustr::Ustr;

pub(crate) const ACCOUNT_ID: &str = "POLYMARKET-001";

/// Production owner field shape (the L2 API key, a UUID4 string). Matches the
/// value `PolymarketClobHttpClient::post_order` injects via `credential.api_key()`,
/// not the maker wallet address.
pub(crate) const API_KEY: &str = "00000000-0000-0000-0000-000000000001";
pub(crate) const API_SECRET_B64: &str = "dGVzdC1zZWNyZXQtMzItYnl0ZXMtbG9uZy12YWx1ZS0wMQ==";
pub(crate) const PASSPHRASE: &str = "test-passphrase";

#[must_use]
pub(crate) fn bench_credential() -> Credential {
    Credential::new(API_KEY, API_SECRET_B64, PASSPHRASE.to_string()).unwrap()
}

/// Token (asset) id used across every WS fixture below.
pub(crate) const YES_TOKEN_ID: &str =
    "71321045679252212594626385532706912750332728571942532289631379312455583992563";
pub(crate) const CONDITION_ID: &str =
    "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917";

#[must_use]
pub(crate) fn account_id() -> AccountId {
    AccountId::from(ACCOUNT_ID)
}

#[must_use]
pub(crate) fn yes_instrument() -> InstrumentAny {
    binary_option(YES_TOKEN_ID, "Yes")
}

#[must_use]
pub(crate) fn yes_instrument_id() -> InstrumentId {
    yes_instrument().id()
}

fn binary_option(token_id: &str, outcome: &str) -> InstrumentAny {
    let symbol_str = format!("{CONDITION_ID}-{token_id}");
    let symbol = Symbol::new(&symbol_str);
    let raw_symbol = Symbol::new(token_id);
    let instrument_id = InstrumentId::new(symbol, *POLYMARKET_VENUE);

    let binary = BinaryOption::new(
        instrument_id,
        raw_symbol,
        AssetClass::Alternative,
        Currency::pUSD(),
        UnixNanos::default(),
        UnixNanos::default(),
        2, // price_precision: tick 0.01 for this token
        6, // size_precision: 6-decimal collateral increments
        Price::from("0.01"),
        Quantity::from("0.000001"),
        Some(Ustr::from(outcome)),
        Some(Ustr::from("bench-question")),
        None,
        None,
        None,
        None,
        Some(Price::from("0.999")),
        Some(Price::from("0.001")),
        None,
        None,
        None,
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    );

    InstrumentAny::BinaryOption(binary)
}

/// Asset-id keyed instrument cache, matching how the execution client looks up
/// instruments from the token id embedded in WS messages.
#[must_use]
pub(crate) fn instrument_cache() -> AHashMap<Ustr, InstrumentAny> {
    let mut cache = AHashMap::new();
    cache.insert(Ustr::from(YES_TOKEN_ID), yes_instrument());
    cache
}

#[must_use]
pub(crate) fn instrument_precisions() -> (u8, u8) {
    let inst = yes_instrument();
    let price_precision = match &inst {
        InstrumentAny::BinaryOption(b) => b.price_precision,
        _ => unreachable!(),
    };
    let size_precision = match &inst {
        InstrumentAny::BinaryOption(b) => b.size_precision,
        _ => unreachable!(),
    };
    (price_precision, size_precision)
}

pub(crate) mod fixtures {
    //! Inline WS / REST frame strings shaped exactly like the venue wire
    //! format. Each fixture exercises one envelope or report variant
    //! end-to-end and is kept small enough to be obvious at a glance.

    /// WS market `book` snapshot (tagged with `event_type: book`).
    pub(crate) const MARKET_BOOK: &str = r#"{
        "event_type": "book",
        "market": "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
        "asset_id": "71321045679252212594626385532706912750332728571942532289631379312455583992563",
        "bids": [
            {"price": "0.46", "size": "1200.0"},
            {"price": "0.47", "size": "800.0"},
            {"price": "0.48", "size": "500.0"},
            {"price": "0.49", "size": "350.0"},
            {"price": "0.50", "size": "200.0"}
        ],
        "asks": [
            {"price": "0.55", "size": "1500.0"},
            {"price": "0.54", "size": "900.0"},
            {"price": "0.53", "size": "400.0"},
            {"price": "0.52", "size": "250.0"},
            {"price": "0.51", "size": "150.0"}
        ],
        "timestamp": "1703875200000"
    }"#;

    /// WS market `price_change` (tagged with `event_type: price_change`).
    ///
    /// Single-change frame: production splits each `price_changes` entry into
    /// its own one-element `PolymarketQuotes` and calls `parse_book_deltas`
    /// per change (see `src/data.rs` `MarketWsMessage::PriceChange` handler),
    /// so the bench unit is one change, not the multi-change envelope.
    pub(crate) const MARKET_PRICE_CHANGE: &str = r#"{
        "event_type": "price_change",
        "market": "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
        "price_changes": [
            {
                "asset_id": "71321045679252212594626385532706912750332728571942532289631379312455583992563",
                "price": "0.51",
                "side": "BUY",
                "size": "150.0",
                "hash": "0xhash001",
                "best_bid": "0.51",
                "best_ask": "0.52"
            }
        ],
        "timestamp": "1703875201000"
    }"#;

    /// WS market `last_trade_price` (tagged with `event_type: last_trade_price`).
    pub(crate) const MARKET_LAST_TRADE: &str = r#"{
        "event_type": "last_trade_price",
        "market": "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
        "asset_id": "71321045679252212594626385532706912750332728571942532289631379312455583992563",
        "fee_rate_bps": "0",
        "price": "0.51",
        "side": "BUY",
        "size": "25.0",
        "timestamp": "1703875202000"
    }"#;

    /// HTTP REST `GET /orders` row used by `parse_order_status_report`.
    pub(crate) const HTTP_OPEN_ORDER: &str = r#"{
        "associate_trades": ["0xabc001"],
        "id": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12",
        "status": "LIVE",
        "market": "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
        "original_size": "100.0000",
        "outcome": "Yes",
        "maker_address": "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
        "owner": "00000000-0000-0000-0000-000000000001",
        "price": "0.5000",
        "side": "BUY",
        "size_matched": "25.0000",
        "asset_id": "71321045679252212594626385532706912750332728571942532289631379312455583992563",
        "expiration": null,
        "order_type": "GTC",
        "created_at": 1703875200
    }"#;

    /// HTTP REST `GET /trades` row used by `parse_fill_report`.
    pub(crate) const HTTP_TRADE_REPORT: &str = r#"{
        "id": "trade-0xabcdef1234",
        "taker_order_id": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12",
        "market": "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
        "asset_id": "71321045679252212594626385532706912750332728571942532289631379312455583992563",
        "side": "BUY",
        "size": "25.0000",
        "fee_rate_bps": "0",
        "price": "0.5000",
        "status": "CONFIRMED",
        "match_time": "2024-01-01T00:00:00Z",
        "last_update": "2024-01-01T00:01:00Z",
        "outcome": "Yes",
        "bucket_index": 0,
        "owner": "00000000-0000-0000-0000-000000000001",
        "maker_address": "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
        "transaction_hash": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890ab",
        "maker_orders": [
            {
                "asset_id": "71321045679252212594626385532706912750332728571942532289631379312455583992563",
                "fee_rate_bps": "0",
                "maker_address": "0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
                "matched_amount": "25.0000",
                "order_id": "0xmaker01maker01maker01maker01maker01maker01maker01maker01maker01maker01",
                "outcome": "Yes",
                "owner": "00000000-0000-0000-0000-000000000002",
                "price": "0.5000",
                "side": "SELL"
            }
        ],
        "trader_side": "TAKER"
    }"#;
}
