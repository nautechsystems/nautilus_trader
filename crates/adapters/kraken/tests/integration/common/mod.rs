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

//! Shared fixtures for the Kraken dispatch integration tests.
//!
//! Rust compiles each file in `tests/` as a standalone binary, so files placed
//! here under `tests/integration/common/` are only pulled in via `mod common;` from the
//! per-product dispatch test files.

#![allow(dead_code)]

use std::sync::Arc;

use nautilus_common::messages::ExecutionEvent;
use nautilus_core::{AtomicMap, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_kraken::websocket::{
    dispatch::{self, OrderIdentity, WsDispatchState},
    spot_v2::messages::KrakenWsExecutionData,
};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{AccountType, OrderSide, OrderType},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId},
    instruments::{Instrument, InstrumentAny, currency_pair::CurrencyPair},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;

/// Venue order ID of the stop-loss-limit buy in `ws_executions_external_orders.json`.
pub(crate) const CAPTURED_STOP_ORDER_ID: &str = "OCAP14-STLMT-000014";

/// Venue order ID of the market sell in `ws_executions_external_orders.json`.
pub(crate) const CAPTURED_MARKET_ORDER_ID: &str = "OCAP15-MARKT-000015";

/// Venue order ID of the limit buy in `ws_executions_order_snapshot.json`.
pub(crate) const CAPTURED_LIMIT_ORDER_ID: &str = "OCAP16-LIMIT-000016";

pub(crate) fn test_emitter() -> (
    ExecutionEventEmitter,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) {
    let clock = get_atomic_clock_realtime();
    let mut emitter = ExecutionEventEmitter::new(
        clock,
        TraderId::from("TESTER-001"),
        AccountId::from("KRAKEN-001"),
        AccountType::Margin,
        None,
    );
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    emitter.set_sender(tx);
    (emitter, rx)
}

pub(crate) fn drain_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> Vec<ExecutionEvent> {
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    events
}

pub(crate) fn account_id() -> AccountId {
    AccountId::from("KRAKEN-001")
}

pub(crate) fn make_identity(
    instrument_id: &str,
    side: OrderSide,
    order_type: OrderType,
) -> OrderIdentity {
    OrderIdentity {
        strategy_id: StrategyId::from("EXEC_TESTER-001"),
        instrument_id: InstrumentId::from(instrument_id),
        order_side: side,
        order_type,
        quantity: Quantity::from("0.0001"),
    }
}

pub(crate) fn empty_string_map() -> Arc<AtomicMap<String, ClientOrderId>> {
    Arc::new(AtomicMap::new())
}

pub(crate) fn empty_instrument_id_map() -> Arc<AtomicMap<String, InstrumentId>> {
    Arc::new(AtomicMap::new())
}

pub(crate) fn empty_quantity_map() -> Arc<AtomicMap<String, Quantity>> {
    Arc::new(AtomicMap::new())
}

pub(crate) fn empty_decimal_map() -> Arc<AtomicMap<String, Decimal>> {
    Arc::new(AtomicMap::new())
}

/// The executions frames of one order in `ws_executions_external_orders.json`, in arrival order.
pub(crate) fn captured_spot_frames(order_id: &str) -> Vec<KrakenWsExecutionData> {
    executions_entries(include_str!(
        "../../../test_data/ws_executions_external_orders.json"
    ))
    .into_iter()
    .filter(|exec| exec.order_id == order_id)
    .collect()
}

/// The executions frames in `ws_executions_order_snapshot.json`, in arrival order: `pending_new`,
/// `new`, `amended`, the `snapshot` entry, then `canceled`.
pub(crate) fn captured_snapshot_frames() -> Vec<KrakenWsExecutionData> {
    executions_entries(include_str!(
        "../../../test_data/ws_executions_order_snapshot.json"
    ))
}

fn executions_entries(json: &str) -> Vec<KrakenWsExecutionData> {
    let envelopes: Vec<serde_json::Value> = serde_json::from_str(json).unwrap();
    envelopes
        .iter()
        .map(|envelope| serde_json::from_value(envelope["data"][0].clone()).unwrap())
        .collect()
}

/// The USDC/EUR pair the captured frames trade.
pub(crate) fn usdc_eur_pair() -> InstrumentAny {
    InstrumentAny::CurrencyPair(
        CurrencyPair::builder()
            .instrument_id(InstrumentId::from("USDC/EUR.KRAKEN"))
            .raw_symbol(Symbol::from("USDCEUR"))
            .base_currency(Currency::USDC())
            .quote_currency(Currency::EUR())
            .price_precision(4)
            .size_precision(8)
            .price_increment(Price::from("0.0001"))
            .size_increment(Quantity::from("0.00000001"))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap(),
    )
}

/// Dispatches each frame through the spot execution dispatch and returns the events each emits.
pub(crate) fn dispatch_spot_frames(
    frames: &[KrakenWsExecutionData],
    state: &WsDispatchState,
    instrument: &InstrumentAny,
) -> Vec<Vec<ExecutionEvent>> {
    let (emitter, mut rx) = test_emitter();
    let instruments = Arc::new(AtomicMap::new());
    instruments.insert(instrument.id(), instrument.clone());
    let truncated_id_map = empty_string_map();
    let order_qty_cache = empty_decimal_map();

    frames
        .iter()
        .map(|exec| {
            dispatch::spot::execution(
                exec,
                state,
                &emitter,
                &instruments,
                &truncated_id_map,
                &order_qty_cache,
                account_id(),
                UnixNanos::default(),
            );
            drain_events(&mut rx)
        })
        .collect()
}
