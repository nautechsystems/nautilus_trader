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

//! Shared bench fixtures for the Lighter adapter.
//!
//! Two surfaces are exposed:
//!
//! - **Signing fixtures** (private key, nonce, hashed message, sample tx
//!   bodies) used by `signing_*` benches in this directory.
//! - **Data / exec pipeline fixtures** (canonical inbound wire strings,
//!   perp instruments, account ids) used by `data.rs`, `exec.rs`, and
//!   `micros.rs`. These mirror the Hyperliquid canonical shape so adapter
//!   benches stay comparable.
//!
//! Wire fixtures are inline `&'static str` consts kept self-contained from
//! the `test_data/` JSON files; venue captures there are reserved for parser
//! correctness tests.

#![allow(dead_code)]

use ahash::AHashMap;
use nautilus_core::{AtomicTime, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_lighter::{
    common::consts::LIGHTER_VENUE,
    signing::{
        curve::{SCALAR_BYTES, Scalar},
        field::{Fp, Fp5},
        schnorr::{PrivateKey, PublicKey, Signature},
        tx::{
            CancelOrderTxInfo, CreateOrderTxInfo, L2TxAttributes, OrderInfo, SignedTx, TxContext,
            sign_tx,
        },
    },
};
use nautilus_model::{
    identifiers::{AccountId, InstrumentId, Symbol, TraderId},
    instruments::{CryptoPerpetual, InstrumentAny},
    types::{Currency, Price, Quantity},
};

// ----- Signing fixtures ---------------------------------------------------

/// Lighter L2 mainnet chain id, used by every `compute_tx_hash` / `sign_tx`
/// fixture so the bench numbers reflect the production hash domain.
pub(crate) const CHAIN_ID: u32 = 304;

pub(crate) fn fixed_sk() -> PrivateKey {
    let bytes: [u8; SCALAR_BYTES] = [
        0x0b, 0x8e, 0x0f, 0x63, 0xc2, 0x4d, 0x8b, 0xaa, 0xcd, 0x9d, 0x29, 0xad, 0x4e, 0x9a, 0x4b,
        0x73, 0xc4, 0xa8, 0xd2, 0xbb, 0x8b, 0x16, 0xdc, 0x4f, 0xa9, 0xd7, 0xc2, 0xe1, 0xd3, 0xa8,
        0xb1, 0xf0, 0xe8, 0xd3, 0xa4, 0xc5, 0xb6, 0xe7, 0xf0, 0x01,
    ];
    PrivateKey::from_le_bytes_reduce(bytes)
}

pub(crate) fn fixed_pk() -> PublicKey {
    fixed_sk().public_key()
}

pub(crate) fn fixed_k() -> Scalar {
    let mut bytes = [0u8; SCALAR_BYTES];
    bytes[0] = 0x42;
    bytes[7] = 0x01;
    bytes[16] = 0x91;
    bytes[24] = 0x37;
    Scalar::from_le_bytes_reduce(bytes)
}

pub(crate) fn fixed_hashed_msg() -> Fp5 {
    Fp5::from_u64s_reduce([
        0x0123_4567_89AB_CDEF,
        0xFEDC_BA98_7654_3210,
        0x1111_2222_3333_4444,
        0x5555_6666_7777_8888,
        0x0000_0001_0000_0001,
    ])
}

pub(crate) fn fixed_signature() -> Signature {
    fixed_sk().sign(fixed_hashed_msg(), fixed_k())
}

pub(crate) fn create_order_tx() -> CreateOrderTxInfo {
    CreateOrderTxInfo {
        context: TxContext {
            account_index: 12345,
            api_key_index: 5,
            nonce: 42,
            expired_at: 1_777_809_907_000,
        },
        order: OrderInfo {
            market_index: 1,
            client_order_index: 7,
            base_amount: 1_000_000,
            price: 25_000_000,
            is_ask: false,
            order_type: 0,
            time_in_force: 0,
            reduce_only: false,
            trigger_price: 0,
            order_expiry: 0,
        },
        attributes: L2TxAttributes::default(),
    }
}

pub(crate) fn cancel_order_tx() -> CancelOrderTxInfo {
    CancelOrderTxInfo {
        context: TxContext {
            account_index: 12345,
            api_key_index: 5,
            nonce: 43,
            expired_at: 1_777_809_907_000,
        },
        market_index: 1,
        index: 7,
        skip_nonce: 0,
    }
}

pub(crate) fn fp_inputs() -> (Fp, Fp) {
    (
        Fp::from_u64_reduce(0x9E37_79B9_7F4A_7C15),
        Fp::from_u64_reduce(0xBB67_AE85_84CA_A73B),
    )
}

pub(crate) fn fp5_inputs() -> (Fp5, Fp5) {
    (
        Fp5::from_u64s_reduce([
            0x9E37_79B9_7F4A_7C15,
            0xBB67_AE85_84CA_A73B,
            0x3C6E_F372_FE94_F82B,
            0xA54F_F53A_5F1D_36F1,
            0x510E_527F_ADE6_82D1,
        ]),
        Fp5::from_u64s_reduce([
            0xCBBB_9D5D_C105_9ED8,
            0x629A_292A_367C_D507,
            0x9159_015A_3070_DD17,
            0x152F_ECD8_F70E_5939,
            0x6712_6F22_38D5_C9F8,
        ]),
    )
}

pub(crate) fn fixed_signed_tx() -> SignedTx {
    sign_tx(&create_order_tx(), CHAIN_ID, &fixed_sk(), fixed_k())
}

// ----- Data / exec pipeline fixtures --------------------------------------

pub(crate) const TRADER_ID: &str = "BENCH-001";
pub(crate) const ACCOUNT_ID: &str = "LIGHTER-001";
pub(crate) const BENCH_ACCOUNT_INDEX: i64 = 1234;
pub(crate) const ETH_MARKET_INDEX: i16 = 0;
pub(crate) const BTC_MARKET_INDEX: i16 = 1;

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
pub(crate) fn eth_perp() -> InstrumentAny {
    perp_instrument("ETH", 2, 4)
}

#[must_use]
pub(crate) fn btc_perp() -> InstrumentAny {
    perp_instrument("BTC", 2, 4)
}

fn perp_instrument(coin: &str, price_precision: u8, size_precision: u8) -> InstrumentAny {
    let raw_symbol = Symbol::new(coin);
    let instrument_id = InstrumentId::new(raw_symbol, *LIGHTER_VENUE);
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

/// Maps market index -> instrument for hot-path lookups in the data benches.
#[must_use]
pub(crate) fn instrument_cache() -> AHashMap<i16, InstrumentAny> {
    let mut cache = AHashMap::new();
    cache.insert(ETH_MARKET_INDEX, eth_perp());
    cache.insert(BTC_MARKET_INDEX, btc_perp());
    cache
}

pub(crate) mod fixtures {
    //! Inline WS frame strings shaped exactly like the venue wire format.
    //! Each fixture exercises one [`LighterWsFrame`] variant end-to-end and
    //! is kept small so the cost being measured stays attributable to one
    //! message kind. The shapes are reproduced from the corresponding
    //! `test_data/ws_*.json` venue captures.
    //!
    //! [`LighterWsFrame`]: nautilus_lighter::websocket::messages::LighterWsFrame

    pub(crate) const TRADE_UPDATE: &str = r#"{
        "channel": "trade:0",
        "liquidation_trades": [],
        "nonce": 8630448841,
        "trades": [{
            "trade_id": 16164557907,
            "trade_id_str": "16164557907",
            "tx_hash": "019f2b9c",
            "type": "trade",
            "market_id": 0,
            "size": "0.1336",
            "price": "2181.83",
            "usd_amount": "291.492488",
            "ask_id": 281476612587355,
            "ask_id_str": "281476612587355",
            "bid_id": 562948334068259,
            "bid_id_str": "562948334068259",
            "ask_client_id": 363283,
            "ask_client_id_str": "363283",
            "bid_client_id": 23004521241,
            "bid_client_id_str": "23004521241",
            "ask_account_id": 57890,
            "bid_account_id": 317068,
            "is_maker_ask": false,
            "block_height": 198321831,
            "timestamp": 1773854156654,
            "taker_fee": 196,
            "maker_fee": 28,
            "transaction_time": 1773854156686065
        }],
        "type": "update/trade"
    }"#;

    pub(crate) const BOOK_UPDATE: &str = r#"{
        "channel": "order_book:0",
        "last_updated_at": 1774884082309144,
        "offset": 1558300,
        "order_book": {
            "code": 0,
            "asks": [
                {"price": "2064.54", "size": "0.3285"},
                {"price": "2064.55", "size": "1.2000"},
                {"price": "2064.56", "size": "0.5500"},
                {"price": "2064.57", "size": "3.1000"},
                {"price": "2064.58", "size": "0.7500"}
            ],
            "bids": [
                {"price": "2064.30", "size": "1.0392"},
                {"price": "2064.29", "size": "0.5000"},
                {"price": "2064.28", "size": "2.2000"},
                {"price": "2064.27", "size": "0.8500"},
                {"price": "2064.26", "size": "1.5000"}
            ],
            "offset": 1558300,
            "nonce": 9182390020,
            "last_updated_at": 1774884082309144,
            "begin_nonce": 9182389998
        },
        "timestamp": 1774884082326,
        "type": "update/order_book"
    }"#;

    pub(crate) const BOOK_SNAPSHOT: &str = r#"{
        "channel": "order_book:0",
        "last_updated_at": 1778138389655150,
        "offset": 2164,
        "order_book": {
            "code": 0,
            "asks": [
                {"price": "2325.00", "size": "99.0000"},
                {"price": "2341.25", "size": "340.1028"},
                {"price": "2342.00", "size": "5.0000"},
                {"price": "2343.50", "size": "10.0000"},
                {"price": "2344.75", "size": "2.5000"}
            ],
            "bids": [
                {"price": "2000.00", "size": "0.0200"},
                {"price": "1999.50", "size": "1.5000"},
                {"price": "1998.00", "size": "3.0000"},
                {"price": "1997.25", "size": "2.0000"},
                {"price": "1996.00", "size": "0.7500"}
            ],
            "offset": 2164,
            "nonce": 904845,
            "last_updated_at": 1778138389655150,
            "begin_nonce": 0
        },
        "timestamp": 1778138582602,
        "type": "subscribed/order_book"
    }"#;

    pub(crate) const TICKER_UPDATE: &str = r#"{
        "channel": "ticker:0",
        "last_updated_at": 1774883844921166,
        "nonce": 9182390020,
        "ticker": {
            "s": "ETH",
            "a": {"price": "2064.48", "size": "0.4950"},
            "b": {"price": "2064.30", "size": "1.0392"},
            "last_updated_at": 1774883844921166
        },
        "timestamp": 1774883844933,
        "type": "update/ticker"
    }"#;

    pub(crate) const CANDLE_UPDATE: &str = r#"{
        "candles": [{
            "t": 1778821440000,
            "o": 2264.2,
            "h": 2264.34,
            "l": 2263.36,
            "c": 2263.89,
            "v": 13.2237,
            "V": 29935.95,
            "i": 19993574218
        }],
        "channel": "candle:0:1m",
        "timestamp": 1778821473331,
        "type": "update/candle"
    }"#;

    pub(crate) const MARKET_STATS_SINGLE: &str = r#"{
        "channel": "market_stats:0",
        "market_stats": {
            "symbol": "ETH",
            "market_id": 0,
            "index_price": "2064.48",
            "mark_price": "2064.47",
            "mid_price": "2064.39",
            "open_interest": "27250.8411",
            "open_interest_limit": "50000.0000",
            "funding_clamp_small": "0.0001",
            "funding_clamp_big": "0.0002",
            "last_trade_price": "2064.50",
            "current_funding_rate": "0.000001",
            "funding_rate": "0.000002",
            "funding_timestamp": 1774886400000,
            "daily_base_token_volume": 199958.6931,
            "daily_quote_token_volume": 471193598.847246,
            "daily_price_low": 2311.81,
            "daily_price_high": 2398.0,
            "daily_price_change": 0.1685414778023213
        },
        "timestamp": 1774883844933,
        "type": "update/market_stats"
    }"#;

    pub(crate) const ACCOUNT_ORDERS_UPDATE: &str = r#"{
        "type": "update/account_orders",
        "channel": "account_orders:0:1234",
        "account": 1234,
        "nonce": 9182390020,
        "orders": {
            "0": [{
                "order_index": 281476929510110,
                "client_order_index": 42,
                "order_id": "281476929510110",
                "client_order_id": "42",
                "market_index": 0,
                "owner_account_index": 1234,
                "initial_base_amount": "0.0050",
                "price": "2352.74",
                "nonce": 9182390020,
                "remaining_base_amount": "0.0030",
                "is_ask": true,
                "base_size": 50,
                "base_price": 235274,
                "filled_base_amount": "0.0020",
                "filled_quote_amount": "4.705480",
                "side": "sell",
                "type": "limit",
                "time_in_force": "good-till-time",
                "reduce_only": false,
                "trigger_price": "0.00",
                "order_expiry": 1780360584479,
                "status": "open",
                "trigger_status": "na",
                "trigger_time": 0,
                "parent_order_index": 0,
                "parent_order_id": "0",
                "to_trigger_order_id_0": "0",
                "to_trigger_order_id_1": "0",
                "to_cancel_order_id_0": "0",
                "integrator_fee_collector_index": "0",
                "integrator_taker_fee": "0",
                "integrator_maker_fee": "0",
                "block_height": 227535532,
                "timestamp": 1777941383576,
                "created_at": 1777941383576,
                "updated_at": 1777941383900,
                "transaction_time": 1777941383576735
            }]
        }
    }"#;

    pub(crate) const ACCOUNT_ALL_TRADES_UPDATE: &str = r#"{
        "type": "update/account_all_trades",
        "channel": "account_all_trades:1234",
        "trades": {
            "0": [{
                "trade_id": 19209006902,
                "trade_id_str": "19209006902",
                "tx_hash": "000000128b1ee814",
                "type": "trade",
                "market_id": 0,
                "size": "0.1336",
                "price": "2352.73",
                "usd_amount": "314.324728",
                "ask_id": 281476929510102,
                "ask_id_str": "281476929510102",
                "bid_id": 562947905631053,
                "bid_id_str": "562947905631053",
                "ask_client_id": 0,
                "ask_client_id_str": "0",
                "bid_client_id": 7001011966,
                "bid_client_id_str": "7001011966",
                "ask_account_id": 91249,
                "bid_account_id": 1234,
                "is_maker_ask": true,
                "block_height": 227535535,
                "timestamp": 1777941384181,
                "taker_fee": 196,
                "maker_fee": 28,
                "transaction_time": 1777941384181586
            }]
        }
    }"#;
}
