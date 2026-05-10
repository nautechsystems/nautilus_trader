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

//! WebSocket message dispatch for the dYdX execution client.
//!
//! Routes incoming subaccount channel data to the appropriate parsing and
//! event emission paths. Tracked orders (submitted through this client) produce
//! proper order events; untracked orders fall back to execution reports for
//! downstream reconciliation.

use std::sync::atomic::{AtomicBool, Ordering};

use dashmap::{DashMap, DashSet};
use nautilus_core::UUID4;
use nautilus_model::{
    enums::{OrderSide, OrderType},
    events::OrderFilled,
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
    reports::FillReport,
    types::Currency,
};

const DEDUP_CAPACITY: usize = 10_000;

/// Order identity context stored at submission time, used by the WS dispatch
/// task to produce proper order events without Cache access.
#[derive(Debug, Clone)]
pub struct OrderIdentity {
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
}

/// Shared state for cross-stream event deduplication in the execution
/// dispatch loop.
#[derive(Debug)]
pub struct DydxWsDispatchState {
    pub order_identities: DashMap<ClientOrderId, OrderIdentity>,
    pub emitted_accepted: DashSet<ClientOrderId>,
    pub filled_orders: DashSet<ClientOrderId>,
    clearing_accepted: AtomicBool,
    clearing_filled: AtomicBool,
}

impl Default for DydxWsDispatchState {
    fn default() -> Self {
        Self {
            order_identities: DashMap::new(),
            emitted_accepted: DashSet::default(),
            filled_orders: DashSet::default(),
            clearing_accepted: AtomicBool::new(false),
            clearing_filled: AtomicBool::new(false),
        }
    }
}

impl DydxWsDispatchState {
    fn evict_if_full(set: &DashSet<ClientOrderId>, flag: &AtomicBool) {
        if set.len() >= DEDUP_CAPACITY
            && flag
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
        {
            set.clear();
            flag.store(false, Ordering::Release);
        }
    }

    pub fn insert_accepted(&self, cid: ClientOrderId) {
        Self::evict_if_full(&self.emitted_accepted, &self.clearing_accepted);
        self.emitted_accepted.insert(cid);
    }

    pub fn insert_filled(&self, cid: ClientOrderId) {
        Self::evict_if_full(&self.filled_orders, &self.clearing_filled);
        self.filled_orders.insert(cid);
    }

    /// Removes an order from all tracking sets after it reaches terminal state.
    pub fn cleanup_terminal(&self, client_order_id: &ClientOrderId) {
        self.order_identities.remove(client_order_id);
        self.emitted_accepted.remove(client_order_id);
        self.filled_orders.remove(client_order_id);
    }
}

/// Converts a [`FillReport`] to an [`OrderFilled`] event for tracked orders.
///
/// Uses the stored [`OrderIdentity`] to supply `order_side` and `order_type`
/// fields that are not available in the report.
///
/// # Panics
///
/// Panics if `report.client_order_id` is `None`.
pub fn fill_report_to_order_filled(
    report: &FillReport,
    trader_id: TraderId,
    identity: &OrderIdentity,
    quote_currency: Currency,
) -> OrderFilled {
    OrderFilled::new(
        trader_id,
        identity.strategy_id,
        report.instrument_id,
        report
            .client_order_id
            .expect("tracked order has client_order_id"),
        report.venue_order_id,
        report.account_id,
        report.trade_id,
        identity.order_side,
        identity.order_type,
        report.last_qty,
        report.last_px,
        quote_currency,
        report.liquidity_side,
        UUID4::new(),
        report.ts_event,
        report.ts_init,
        false,
        report.venue_position_id,
        Some(report.commission),
    )
}
