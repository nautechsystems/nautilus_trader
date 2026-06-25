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

//! Shared OCM stream handler state.

use ahash::{AHashMap, AHashSet};
use nautilus_model::identifiers::{ClientOrderId, StrategyId};
use rust_decimal::Decimal;

use crate::{
    common::{
        parse::{make_customer_order_ref, make_customer_order_ref_legacy},
        types::OrderSyncEntry,
    },
    stream::parse::FillTracker,
};

/// Shared mutable state for the OCM stream handler.
///
/// Accessed by both the TCP reader closure and the execution client methods
/// (submit, modify, connect/disconnect). All access goes through `Arc<Mutex<>>`.
#[derive(Debug, Default)]
pub struct OcmState {
    pub fill_tracker: FillTracker,
    /// Maps customer_order_ref (rfo) to ClientOrderId for stream resolution.
    pub customer_order_refs: AHashMap<String, ClientOrderId>,
    /// Maps client_order_id to submitting strategy. Captured at submit so the stream task
    /// builds direct events for tracked orders without cache access.
    pub order_strategies: AHashMap<ClientOrderId, StrategyId>,
    /// Client order IDs that already had an `OrderAccepted` emitted (via the HTTP
    /// place response or stream synthesis), so acceptance is applied exactly once.
    pub accepted_orders: AHashSet<ClientOrderId>,
    /// Client order IDs that already received an OCM order status update.
    pub stream_reported_client_orders: AHashSet<ClientOrderId>,
    /// Bet IDs that have received a terminal event (cancel, lapse, fill-complete).
    pub terminal_orders: AHashSet<String>,
    /// Old bet IDs from replace operations, to suppress late stream updates.
    pub replaced_venue_order_ids: AHashSet<String>,
    /// (client_order_id, old_bet_id) pairs for in-flight replace operations.
    pub pending_update_keys: AHashSet<(ClientOrderId, String)>,
}

impl OcmState {
    /// Registers a customer_order_ref mapping for a new order.
    pub fn register_customer_order_ref(&mut self, client_order_id: ClientOrderId) {
        let rfo = make_customer_order_ref(client_order_id.as_str());
        self.customer_order_refs.insert(rfo, client_order_id);
    }

    /// Registers both current and legacy customer_order_ref truncations.
    ///
    /// Used during reconnect sync for pre-existing orders that may
    /// have been placed with either truncation format.
    pub fn register_customer_order_ref_with_legacy(&mut self, client_order_id: ClientOrderId) {
        let rfo = make_customer_order_ref(client_order_id.as_str());
        let rfo_legacy = make_customer_order_ref_legacy(client_order_id.as_str());
        self.customer_order_refs.insert(rfo, client_order_id);

        if rfo_legacy != client_order_id.as_str() {
            self.customer_order_refs.insert(rfo_legacy, client_order_id);
        }
    }

    /// Records the submitting strategy for a tracked order.
    pub fn register_order_identity(
        &mut self,
        client_order_id: ClientOrderId,
        strategy_id: StrategyId,
    ) {
        self.order_strategies.insert(client_order_id, strategy_id);
    }

    /// Returns the submitting strategy for a tracked order, if known.
    pub fn order_strategy_id(&self, client_order_id: &ClientOrderId) -> Option<StrategyId> {
        self.order_strategies.get(client_order_id).copied()
    }

    /// Records that acceptance has been emitted for a tracked order.
    ///
    /// Returns `true` when this call newly marks the order accepted (the caller
    /// should emit `OrderAccepted`), or `false` when acceptance was already emitted.
    pub fn mark_accepted(&mut self, client_order_id: ClientOrderId) -> bool {
        self.accepted_orders.insert(client_order_id)
    }

    /// Removes customer_order_ref mappings for a client_order_id.
    pub fn remove_customer_order_refs(&mut self, client_order_id: &ClientOrderId) {
        let rfo = make_customer_order_ref(client_order_id.as_str());
        let rfo_legacy = make_customer_order_ref_legacy(client_order_id.as_str());
        self.customer_order_refs.remove(&rfo);
        self.customer_order_refs.remove(&rfo_legacy);
        self.order_strategies.remove(client_order_id);
        self.accepted_orders.remove(client_order_id);
    }

    /// Resolves a client_order_id from the unmatched order's rfo field.
    pub fn resolve_client_order_id(&self, rfo: Option<&str>) -> Option<ClientOrderId> {
        rfo.and_then(|r| self.customer_order_refs.get(r).copied())
    }

    /// Returns `true` if a cancel/lapse for this bet should be suppressed
    /// because a replace operation is pending or the bet was already replaced.
    pub fn should_suppress_cancel(&self, client_order_id: &ClientOrderId, bet_id: &str) -> bool {
        if self.replaced_venue_order_ids.contains(bet_id) {
            return true;
        }

        self.pending_update_keys
            .contains(&(*client_order_id, bet_id.to_string()))
    }

    /// Cleans up customer_order_ref mappings for a terminal order,
    /// unless a pending replace exists for this client_order_id.
    pub fn cleanup_terminal_order(&mut self, client_order_id: &ClientOrderId) {
        let has_pending = self
            .pending_update_keys
            .iter()
            .any(|(cid, _)| cid == client_order_id);

        if !has_pending {
            self.remove_customer_order_refs(client_order_id);
        }
    }

    /// Anchors the fill tracker against cached orders so the post-reconnect
    /// image neither treats cumulative size as a new fill nor re-emits a
    /// fill that was published via another channel.
    pub fn sync_from_orders(&mut self, orders: &[OrderSyncEntry]) {
        for entry in orders {
            if entry.is_closed {
                self.terminal_orders.insert(entry.bet_id.clone());
            } else {
                self.register_customer_order_ref_with_legacy(entry.client_order_id);
            }

            if entry.filled_qty > Decimal::ZERO {
                self.fill_tracker
                    .sync_order(&entry.bet_id, entry.filled_qty, entry.avg_px);
            }

            if !entry.trade_ids.is_empty() {
                self.fill_tracker
                    .seed_published_trade_ids(entry.trade_ids.iter().cloned());
            }
        }
    }
}
