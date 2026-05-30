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

//! Shared utilities for Derive criterion benches.
//!
//! Inbound fixtures are real venue captures from `test_data/`: the inner-data
//! files are wrapped at bench setup by [`subscription_frame`] into the JSON-RPC
//! subscription envelope the live feed delivers, so bench inputs track the
//! parser test corpus. Execution fixtures load the same `DeriveInstrument`,
//! `DeriveOrder`, and `DeriveTrade` records the parser tests use.
//!
//! Each criterion bench is a separate compilation unit that pulls in this
//! module, but uses only a subset of these fixtures and builders. Without the
//! module-level `allow`, the unused subset in any given bench triggers
//! per-crate dead-code warnings.

#![allow(dead_code)]

use nautilus_common::messages::ExecutionEvent;
use nautilus_core::{AtomicTime, time::get_atomic_clock_realtime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::AccountType,
    identifiers::{AccountId, TraderId},
    types::Currency,
};

pub(crate) const TRADER_ID: &str = "BENCH-001";
pub(crate) const ACCOUNT_ID: &str = "DERIVE-001";

/// ETH-PERP price precision: `tick_size` 0.01 -> 2 decimal places.
pub(crate) const PRICE_PRECISION: u8 = 2;
/// ETH-PERP size precision: `amount_step` 0.001 -> 3 decimal places.
pub(crate) const SIZE_PRECISION: u8 = 3;

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

/// Wraps a channel topic and inner data payload in the JSON-RPC subscription
/// envelope that [`nautilus_derive::websocket::messages::DeriveWsFrame::parse`]
/// expects. Built once per bench outside the timed loop so the format cost is
/// not measured.
#[must_use]
pub(crate) fn subscription_frame(channel: &str, data: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","method":"subscription","params":{{"channel":"{channel}","data":{data}}}}}"#
    )
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
        AccountType::Margin,
        Some(Currency::from("USDC")),
    );
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    emitter.set_sender(tx);
    (emitter, rx)
}

pub(crate) mod fixtures {
    //! Inner-data captures from `test_data/`, wrapped at bench setup by
    //! [`super::subscription_frame`] into the live subscription envelope.
    //! Execution records are bare objects deserialized directly.

    /// Order book snapshot data (`orderbook.ETH-PERP.1.10`).
    pub(crate) const ORDERBOOK: &str = include_str!("../../test_data/perps/ws_orderbook_eth.json");
    /// Single public trade (wrapped in an array for the `trades.*` channel).
    pub(crate) const TRADE: &str = include_str!("../../test_data/perps/ws_trade_eth.json");
    /// Perp slim ticker carrying best bid/ask, mark, index, and funding.
    pub(crate) const TICKER_PERP: &str =
        include_str!("../../test_data/perps/ws_ticker_slim_eth.json");
    /// Option slim ticker carrying Black-Scholes greeks under `option_pricing`.
    pub(crate) const TICKER_OPTION: &str =
        include_str!("../../test_data/options/ws_ticker_slim_eth_call.json");
    /// REST OHLCV candle records (`public/get_tradingview_chart_data`).
    pub(crate) const CANDLES: &str =
        include_str!("../../test_data/perps/http_public_candles_eth.json");

    /// ETH-PERP instrument definition consumed by the signed order path.
    pub(crate) const INSTRUMENT_PERP: &str =
        include_str!("../../test_data/perps/instrument_eth.json");
    /// Partially filled order record (`label` = `alpha-strategy`).
    pub(crate) const ORDER: &str =
        include_str!("../../test_data/perps/http_order_eth_partially_filled.json");
    /// Settled private trade record (`label` = `alpha-strategy`).
    pub(crate) const TRADE_PRIVATE: &str =
        include_str!("../../test_data/perps/http_private_trade_eth.json");

    /// Channel topic for [`ORDERBOOK`].
    pub(crate) const ORDERBOOK_CHANNEL: &str = "orderbook.ETH-PERP.1.10";
    /// Channel topic for the trades stream.
    pub(crate) const TRADES_CHANNEL: &str = "trades.perp.ETH";
    /// Channel topic for [`TICKER_PERP`].
    pub(crate) const TICKER_PERP_CHANNEL: &str = "ticker_slim.ETH-PERP.1000";
    /// Channel topic for [`TICKER_OPTION`]; the option symbol drives the parsed instrument id.
    pub(crate) const TICKER_OPTION_CHANNEL: &str = "ticker_slim.ETH-20260627-3500-C.1000";

    /// Client order id carried by [`ORDER`] and [`TRADE_PRIVATE`] via `label`.
    pub(crate) const TRACKED_LABEL: &str = "alpha-strategy";
}
