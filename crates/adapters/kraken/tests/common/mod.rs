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

//! Shared helpers for the Kraken dispatch integration tests.
//!
//! Rust compiles each file in `tests/` as a standalone binary, so files placed
//! here under `tests/common/` are only pulled in via `mod common;` from the
//! per-product dispatch test files.

#![allow(dead_code)]

use std::sync::Arc;

use nautilus_common::messages::ExecutionEvent;
use nautilus_core::{AtomicMap, time::get_atomic_clock_realtime};
use nautilus_kraken::websocket::dispatch::OrderIdentity;
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{AccountType, OrderSide, OrderType},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};

pub fn test_emitter() -> (
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

pub fn drain_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> Vec<ExecutionEvent> {
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    events
}

pub fn account_id() -> AccountId {
    AccountId::from("KRAKEN-001")
}

pub fn make_identity(instrument_id: &str, side: OrderSide, order_type: OrderType) -> OrderIdentity {
    OrderIdentity {
        strategy_id: StrategyId::from("EXEC_TESTER-001"),
        instrument_id: InstrumentId::from(instrument_id),
        order_side: side,
        order_type,
        quantity: Quantity::from("0.0001"),
    }
}

pub fn empty_string_map() -> Arc<AtomicMap<String, ClientOrderId>> {
    Arc::new(AtomicMap::new())
}

pub fn empty_instrument_id_map() -> Arc<AtomicMap<String, InstrumentId>> {
    Arc::new(AtomicMap::new())
}

pub fn empty_quantity_map() -> Arc<AtomicMap<String, Quantity>> {
    Arc::new(AtomicMap::new())
}

pub fn empty_f64_map() -> Arc<AtomicMap<String, f64>> {
    Arc::new(AtomicMap::new())
}
