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

use std::collections::VecDeque;

use ahash::{AHashMap, AHashSet};
use nautilus_model::{
    identifiers::{ClientOrderId, StrategyId},
    types::Quantity,
};
use rust_decimal::Decimal;

use crate::{
    common::{
        parse::{make_customer_order_ref, make_customer_order_ref_legacy},
        types::OrderSyncEntry,
    },
    stream::parse::FillTracker,
};

#[derive(Debug, Default)]
pub(crate) struct PendingReplaceState {
    pub(crate) total_quantity: Option<Quantity>,
    pub(crate) old_terminal: bool,
}

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
    stream_reported_order_queue: VecDeque<ClientOrderId>,
    /// Bet IDs that have received a terminal event (cancel, lapse, fill-complete).
    pub terminal_orders: AHashSet<String>,
    terminal_order_queue: VecDeque<String>,
    /// Old bet IDs from replace operations, to suppress late stream updates.
    pub replaced_venue_order_ids: AHashSet<String>,
    canceled_replace_bet_ids: AHashSet<String>,
    /// (client_order_id, old_bet_id) pairs for in-flight replace operations.
    pub pending_update_keys: AHashSet<(ClientOrderId, String)>,
    pending_replace_state: AHashMap<(ClientOrderId, String), PendingReplaceState>,
}

impl OcmState {
    /// Bounds dedup memory while retaining recent delayed stream and REST overlap.
    const DEDUP_RETENTION: usize = 10_000;

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

    pub(crate) fn mark_stream_reported(&mut self, client_order_id: ClientOrderId) {
        if !self.stream_reported_client_orders.insert(client_order_id) {
            return;
        }

        self.stream_reported_order_queue.push_back(client_order_id);
        if self.stream_reported_order_queue.len() > Self::DEDUP_RETENTION
            && let Some(expired_client_order_id) = self.stream_reported_order_queue.pop_front()
        {
            self.stream_reported_client_orders
                .remove(&expired_client_order_id);
        }
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

    pub(crate) fn register_pending_replace(
        &mut self,
        client_order_id: ClientOrderId,
        old_bet_id: String,
        total_quantity: Option<Quantity>,
    ) {
        let key = (client_order_id, old_bet_id);
        self.pending_update_keys.insert(key.clone());
        self.pending_replace_state.insert(
            key,
            PendingReplaceState {
                total_quantity,
                old_terminal: false,
            },
        );
    }

    pub(crate) fn mark_pending_replace_terminal(
        &mut self,
        client_order_id: ClientOrderId,
        old_bet_id: &str,
    ) {
        let key = (client_order_id, old_bet_id.to_string());
        if self.pending_update_keys.contains(&key) {
            self.pending_replace_state
                .entry(key)
                .or_default()
                .old_terminal = true;
        }
    }

    pub(crate) fn take_pending_replace(
        &mut self,
        client_order_id: ClientOrderId,
        old_bet_id: &str,
    ) -> Option<PendingReplaceState> {
        let key = (client_order_id, old_bet_id.to_string());
        let was_pending = self.pending_update_keys.remove(&key);
        let state = self.pending_replace_state.remove(&key).unwrap_or_default();
        was_pending.then_some(state)
    }

    pub(crate) fn mark_canceled_replace(&mut self, bet_id: String) {
        self.mark_terminal_order(bet_id.clone());
        self.canceled_replace_bet_ids.insert(bet_id);
    }

    pub(crate) fn is_canceled_replace(&self, bet_id: &str) -> bool {
        self.canceled_replace_bet_ids.contains(bet_id)
    }

    pub(crate) fn clear_canceled_replace(&mut self, bet_id: &str) {
        self.canceled_replace_bet_ids.remove(bet_id);
    }

    /// Promotes a new bet observed while a price replacement is pending.
    ///
    /// Returns the total order quantity when `new_bet_id` differs from a pending old bet ID.
    pub(crate) fn promote_pending_replace(
        &mut self,
        client_order_id: &ClientOrderId,
        new_bet_id: &str,
        replacement_quantity: Quantity,
    ) -> Option<Quantity> {
        if self.replaced_venue_order_ids.contains(new_bet_id) {
            return None;
        }

        let old_bet_ids: Vec<String> = self
            .pending_update_keys
            .iter()
            .filter(|(candidate, old_bet_id)| {
                candidate == client_order_id && old_bet_id != new_bet_id
            })
            .map(|(_, old_bet_id)| old_bet_id.clone())
            .collect();

        if old_bet_ids.is_empty() {
            return None;
        }

        let total_quantity = old_bet_ids
            .first()
            .and_then(|old_bet_id| {
                self.pending_replace_state
                    .get(&(*client_order_id, old_bet_id.clone()))
            })
            .and_then(|state| state.total_quantity)
            .unwrap_or(replacement_quantity);

        self.pending_update_keys
            .retain(|(candidate, _)| candidate != client_order_id);
        self.pending_replace_state
            .retain(|(candidate, _), _| candidate != client_order_id);
        self.replaced_venue_order_ids.extend(old_bet_ids);
        Some(total_quantity)
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

    /// Records a terminal bet and bounds the stream and REST dedup state.
    pub fn mark_terminal_order(&mut self, bet_id: String) {
        if !self.terminal_orders.insert(bet_id.clone()) {
            return;
        }

        self.terminal_order_queue.push_back(bet_id);
        if self.terminal_order_queue.len() > Self::DEDUP_RETENTION
            && let Some(expired_bet_id) = self.terminal_order_queue.pop_front()
        {
            self.terminal_orders.remove(&expired_bet_id);
            self.replaced_venue_order_ids.remove(&expired_bet_id);
            self.canceled_replace_bet_ids.remove(&expired_bet_id);
            self.fill_tracker.prune(&expired_bet_id);
        }
    }

    /// Anchors the fill tracker against cached orders so the post-reconnect
    /// image neither treats cumulative size as a new fill nor re-emits a
    /// fill that was published via another channel.
    pub fn sync_from_orders(&mut self, orders: &[OrderSyncEntry]) {
        for entry in orders {
            if entry.is_closed {
                self.mark_terminal_order(entry.bet_id.clone());
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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn terminal_order_retention_evicts_fill_tracker_state() {
        let mut state = OcmState::default();
        let first_bet_id = "bet-0";
        let size_matched = Decimal::new(10, 0);
        let average_price = Decimal::new(20, 1);

        assert!(
            state
                .fill_tracker
                .advance_cumulative_fill(
                    first_bet_id,
                    size_matched,
                    Some(average_price),
                    average_price,
                )
                .is_some(),
        );
        state
            .replaced_venue_order_ids
            .insert(first_bet_id.to_string());
        state.mark_terminal_order(first_bet_id.to_string());
        for index in 1..=OcmState::DEDUP_RETENTION {
            state.mark_terminal_order(format!("bet-{index}"));
        }

        let replay_after_eviction = state.fill_tracker.advance_cumulative_fill(
            first_bet_id,
            size_matched,
            Some(average_price),
            average_price,
        );

        assert!(!state.terminal_orders.contains(first_bet_id));
        assert!(!state.replaced_venue_order_ids.contains(first_bet_id));
        assert!(replay_after_eviction.is_some());
    }

    #[rstest]
    fn stream_reported_order_retention_is_bounded() {
        let mut state = OcmState::default();
        let first_client_order_id = ClientOrderId::from("O-0");

        state.mark_stream_reported(first_client_order_id);
        for index in 1..=OcmState::DEDUP_RETENTION {
            state.mark_stream_reported(ClientOrderId::from(format!("O-{index}")));
        }

        assert_eq!(
            state.stream_reported_client_orders.len(),
            OcmState::DEDUP_RETENTION,
        );
        assert!(
            !state
                .stream_reported_client_orders
                .contains(&first_client_order_id)
        );
    }

    #[rstest]
    fn pending_replace_promotes_only_a_different_bet() {
        let client_order_id = ClientOrderId::from("O-1");
        let mut state = OcmState::default();
        state.register_pending_replace(
            client_order_id,
            "old-bet".to_string(),
            Some(Quantity::from(10)),
        );

        assert_eq!(
            state.promote_pending_replace(&client_order_id, "old-bet", Quantity::from(8)),
            None
        );
        assert_eq!(
            state.promote_pending_replace(&client_order_id, "new-bet", Quantity::from(8)),
            Some(Quantity::from(10)),
        );
        assert!(state.pending_update_keys.is_empty());
        assert!(state.replaced_venue_order_ids.contains("old-bet"));
        assert_eq!(
            state.promote_pending_replace(&client_order_id, "newer-bet", Quantity::from(8)),
            None,
        );
    }

    #[rstest]
    fn pending_replace_does_not_promote_a_historical_bet() {
        let client_order_id = ClientOrderId::from("O-1");
        let mut state = OcmState::default();
        state
            .replaced_venue_order_ids
            .insert("historical-bet".to_string());
        state.register_pending_replace(
            client_order_id,
            "current-bet".to_string(),
            Some(Quantity::from(10)),
        );

        assert_eq!(
            state.promote_pending_replace(&client_order_id, "historical-bet", Quantity::from(8)),
            None,
        );
        assert!(
            state.should_suppress_cancel(&client_order_id, "current-bet"),
            "historical traffic must not consume the pending replace",
        );
        assert_eq!(
            state.promote_pending_replace(&client_order_id, "replacement-bet", Quantity::from(8)),
            Some(Quantity::from(10)),
        );
    }

    #[rstest]
    fn canceled_replace_keeps_one_terminal_retention_entry() {
        let mut state = OcmState::default();

        state.mark_terminal_order("old-bet".to_string());
        state.mark_canceled_replace("old-bet".to_string());
        state.mark_terminal_order("old-bet".to_string());

        assert!(state.terminal_orders.contains("old-bet"));
        assert_eq!(state.terminal_order_queue.len(), 1);
    }

    #[rstest]
    fn canceled_replace_without_ocm_has_terminal_retention_entry() {
        let mut state = OcmState::default();

        state.mark_canceled_replace("old-bet".to_string());

        assert!(state.terminal_orders.contains("old-bet"));
        assert_eq!(state.terminal_order_queue.len(), 1);
    }
}
