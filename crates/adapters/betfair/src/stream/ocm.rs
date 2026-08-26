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
    identifiers::{ClientOrderId, StrategyId, VenueOrderId},
    types::Quantity,
};
use rust_decimal::Decimal;

use crate::{
    common::{
        parse::{make_customer_order_ref, make_customer_order_ref_legacy},
        types::{BetId, CustomerOrderRef, OrderSyncEntry},
    },
    stream::{messages::UnmatchedOrder, parse::FillTracker},
};

#[derive(Clone, Debug)]
pub(crate) struct PendingReplaceState {
    pub(crate) total_quantity: Option<Quantity>,
    awaiting_reconciliation: bool,
}

#[derive(Debug, Clone, Copy)]
struct PendingReductionState {
    client_order_id: ClientOrderId,
    original_quantity: Quantity,
    requested_quantity: Quantity,
    confirmed_quantity: Option<Quantity>,
}

impl PendingReductionState {
    fn is_unconfirmed_for(&self, client_order_id: &ClientOrderId) -> bool {
        self.client_order_id == *client_order_id && self.confirmed_quantity.is_none()
    }

    fn can_confirm(&self, client_order_id: &ClientOrderId, active_quantity: Quantity) -> bool {
        self.is_unconfirmed_for(client_order_id)
            && active_quantity >= self.requested_quantity
            && active_quantity < self.original_quantity
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CustomerOrderRefResolution {
    Unique(ClientOrderId),
    Ambiguous,
}

impl CustomerOrderRefResolution {
    pub(crate) fn client_order_id(self) -> Option<ClientOrderId> {
        match self {
            Self::Unique(client_order_id) => Some(client_order_id),
            Self::Ambiguous => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct OrderCorrelation {
    customer_order_refs: AHashSet<CustomerOrderRef>,
    strategy_id: Option<StrategyId>,
    venue_order_id: Option<VenueOrderId>,
    venue_order_ids: AHashSet<BetId>,
    accepted: bool,
    terminal_retained: bool,
}

#[derive(Debug, Clone)]
enum TerminalRetentionKey {
    Owned(ClientOrderId),
    External(BetId),
}

/// Shared mutable state for the OCM stream handler.
///
/// Accessed by both the TCP reader closure and the execution client methods
/// (submit, modify, connect/disconnect). All access goes through `Arc<Mutex<>>`.
///
/// Terminal retention owns order correlation, per-Bet fill and reduction state, and replacement
/// history. The first REST, OCM, or reconciliation observation with active quantity at least the
/// requested quantity and below the original confirms a pending reduction; later observations are
/// no-ops.
#[derive(Clone, Debug, Default)]
pub struct OcmState {
    /// Tracks cumulative per-bet fill and void state for deduplication and reconciliation.
    pub fill_tracker: FillTracker,
    /// Bet IDs that have received a terminal event (cancel, lapse, fill-complete).
    pub terminal_orders: AHashSet<BetId>,
    /// Old bet IDs from replace operations, to suppress late stream updates.
    pub replaced_venue_order_ids: AHashSet<BetId>,
    pub(crate) customer_order_refs: AHashMap<CustomerOrderRef, CustomerOrderRefResolution>,
    order_correlations: AHashMap<ClientOrderId, OrderCorrelation>,
    terminal_order_queue: VecDeque<TerminalRetentionKey>,
    canceled_replace_bet_ids: AHashSet<BetId>,
    pending_replace_state: AHashMap<(ClientOrderId, BetId), PendingReplaceState>,
    pending_reductions: AHashMap<BetId, PendingReductionState>,
}

impl OcmState {
    /// Bounds dedup memory while retaining recent delayed stream and REST overlap.
    pub(crate) const DEDUP_RETENTION: usize = 10_000;

    pub(crate) fn register_submission(
        &mut self,
        client_order_id: ClientOrderId,
        strategy_id: StrategyId,
    ) -> Result<(), String> {
        self.register_order_ref(client_order_id)?;
        self.register_order_identity(client_order_id, strategy_id);
        Ok(())
    }

    /// Registers a customer_order_ref mapping for a new order.
    pub fn register_customer_order_ref(&mut self, client_order_id: ClientOrderId) {
        let _ = self.register_order_ref(client_order_id);
    }

    /// Registers both current and legacy customer_order_ref truncations.
    pub fn register_customer_order_ref_with_legacy(&mut self, client_order_id: ClientOrderId) {
        let current = make_customer_order_ref(client_order_id.as_str());
        let legacy = make_customer_order_ref_legacy(client_order_id.as_str());
        self.upsert_order_correlation(client_order_id, None, false, [current, legacy]);
    }

    /// Records the submitting strategy for a tracked order.
    pub fn register_order_identity(
        &mut self,
        client_order_id: ClientOrderId,
        strategy_id: StrategyId,
    ) {
        self.order_correlations
            .entry(client_order_id)
            .or_default()
            .strategy_id = Some(strategy_id);
    }

    pub(crate) fn register_order_ref(
        &mut self,
        client_order_id: ClientOrderId,
    ) -> Result<(), String> {
        let customer_order_ref = make_customer_order_ref(client_order_id.as_str());

        if self
            .customer_order_refs
            .get(&customer_order_ref)
            .is_some_and(|resolution| {
                *resolution != CustomerOrderRefResolution::Unique(client_order_id)
            })
        {
            return Err(customer_order_ref);
        }

        self.upsert_order_correlation(client_order_id, None, false, [customer_order_ref]);
        Ok(())
    }

    pub(crate) fn restore_order(
        &mut self,
        client_order_id: ClientOrderId,
        strategy_id: StrategyId,
        venue_order_id: VenueOrderId,
    ) {
        let current = make_customer_order_ref(client_order_id.as_str());
        let legacy = make_customer_order_ref_legacy(client_order_id.as_str());
        self.upsert_order_correlation(client_order_id, Some(strategy_id), true, [current, legacy]);
        self.bind_venue_order_id(&client_order_id, venue_order_id);
    }

    fn upsert_order_correlation(
        &mut self,
        client_order_id: ClientOrderId,
        strategy_id: Option<StrategyId>,
        accepted: bool,
        customer_order_refs: impl IntoIterator<Item = String>,
    ) {
        let correlation = self.order_correlations.entry(client_order_id).or_default();
        if let Some(strategy_id) = strategy_id {
            correlation.strategy_id = Some(strategy_id);
        }

        correlation.accepted |= accepted;
        let mut affected = Vec::new();

        for customer_order_ref in customer_order_refs {
            if correlation
                .customer_order_refs
                .insert(customer_order_ref.clone())
            {
                affected.push(customer_order_ref);
            }
        }

        for customer_order_ref in affected {
            self.add_customer_order_ref(client_order_id, customer_order_ref);
        }
    }

    fn add_customer_order_ref(
        &mut self,
        client_order_id: ClientOrderId,
        customer_order_ref: String,
    ) {
        match self.customer_order_refs.get(&customer_order_ref).copied() {
            None => {
                self.customer_order_refs.insert(
                    customer_order_ref,
                    CustomerOrderRefResolution::Unique(client_order_id),
                );
            }
            Some(CustomerOrderRefResolution::Unique(owner)) if owner != client_order_id => {
                self.customer_order_refs
                    .insert(customer_order_ref, CustomerOrderRefResolution::Ambiguous);
            }
            Some(_) => {}
        }
    }

    /// Returns the submitting strategy for a tracked order, if known.
    pub fn order_strategy_id(&self, client_order_id: &ClientOrderId) -> Option<StrategyId> {
        self.order_correlations
            .get(client_order_id)
            .and_then(|correlation| correlation.strategy_id)
    }

    pub(crate) fn bind_venue_order_id(
        &mut self,
        client_order_id: &ClientOrderId,
        venue_order_id: VenueOrderId,
    ) {
        let bet_id = venue_order_id.to_string();
        let correlation = self.order_correlations.entry(*client_order_id).or_default();
        let newly_correlated = correlation.venue_order_ids.insert(bet_id.clone());
        correlation.venue_order_id = Some(venue_order_id);

        if self.terminal_orders.contains(&bet_id) {
            if newly_correlated {
                self.remove_external_terminal_retention(&bet_id);
            }

            if !self.has_pending_replace(client_order_id) {
                self.retain_terminal_identity(*client_order_id);
            }
        }
    }

    /// Records that acceptance has been emitted for a tracked order.
    ///
    /// Returns `true` when this call newly marks the order accepted (the caller
    /// should emit `OrderAccepted`), or `false` when acceptance was already emitted.
    pub fn mark_accepted(&mut self, client_order_id: ClientOrderId) -> bool {
        let correlation = self.order_correlations.entry(client_order_id).or_default();
        if correlation.accepted {
            return false;
        }

        correlation.accepted = true;
        true
    }

    pub(crate) fn is_accepted(&self, client_order_id: &ClientOrderId) -> bool {
        self.order_correlations
            .get(client_order_id)
            .is_some_and(|correlation| correlation.accepted)
    }

    pub(crate) fn claim_acceptance(
        &mut self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) -> bool {
        if !self.mark_accepted(client_order_id) {
            return false;
        }

        self.bind_venue_order_id(&client_order_id, venue_order_id);
        true
    }

    pub(crate) fn remove_order_correlation(&mut self, client_order_id: &ClientOrderId) {
        self.remove_terminal_retention(client_order_id);
        let Some(correlation) = self.order_correlations.remove(client_order_id) else {
            return;
        };
        let OrderCorrelation {
            customer_order_refs: affected,
            venue_order_ids,
            ..
        } = correlation;

        for customer_order_ref in affected {
            if self
                .customer_order_refs
                .get(&customer_order_ref)
                .is_some_and(|resolution| {
                    *resolution == CustomerOrderRefResolution::Unique(*client_order_id)
                })
            {
                self.customer_order_refs.remove(&customer_order_ref);
            } else {
                self.rebuild_customer_order_ref(&customer_order_ref);
            }
        }

        self.pending_reductions
            .retain(|_, pending| pending.client_order_id != *client_order_id);
        self.pending_replace_state
            .retain(|(candidate, _), _| candidate != client_order_id);

        for bet_id in venue_order_ids {
            self.remove_venue_order_state(&bet_id);
        }
    }

    /// Removes customer_order_ref mappings for a client_order_id.
    pub fn remove_customer_order_refs(&mut self, client_order_id: &ClientOrderId) {
        self.remove_order_correlation(client_order_id);
    }

    fn rebuild_customer_order_ref(&mut self, customer_order_ref: &str) {
        let mut owners = self
            .order_correlations
            .iter()
            .filter(|(_, correlation)| correlation.customer_order_refs.contains(customer_order_ref))
            .map(|(client_order_id, _)| *client_order_id);
        let resolution = owners.next().map(|client_order_id| {
            if owners.next().is_some() {
                CustomerOrderRefResolution::Ambiguous
            } else {
                CustomerOrderRefResolution::Unique(client_order_id)
            }
        });

        if let Some(resolution) = resolution {
            self.customer_order_refs
                .insert(customer_order_ref.to_string(), resolution);
        } else {
            self.customer_order_refs.remove(customer_order_ref);
        }
    }

    pub(crate) fn customer_order_ref_resolution(
        &self,
        customer_order_ref: &str,
    ) -> Option<CustomerOrderRefResolution> {
        self.customer_order_refs.get(customer_order_ref).copied()
    }

    pub(crate) fn resolve_order_owner(
        &self,
        customer_order_ref: Option<&str>,
        venue_order_id: &str,
    ) -> Option<CustomerOrderRefResolution> {
        customer_order_ref
            .and_then(|reference| self.customer_order_ref_resolution(reference))
            .or_else(|| {
                self.client_order_id_by_venue_order_id(venue_order_id)
                    .map(CustomerOrderRefResolution::Unique)
            })
    }

    pub(crate) fn client_order_id_by_venue_order_id(
        &self,
        venue_order_id: &str,
    ) -> Option<ClientOrderId> {
        let mut owners = self
            .order_correlations
            .iter()
            .filter(|(_, correlation)| correlation.venue_order_ids.contains(venue_order_id))
            .map(|(client_order_id, _)| *client_order_id);
        let owner = owners.next()?;
        owners.next().is_none().then_some(owner)
    }

    /// Resolves a client_order_id from the unmatched order's rfo field.
    pub fn resolve_client_order_id(&self, rfo: Option<&str>) -> Option<ClientOrderId> {
        rfo.and_then(|customer_order_ref| {
            self.customer_order_ref_resolution(customer_order_ref)
                .and_then(CustomerOrderRefResolution::client_order_id)
        })
    }

    /// Returns `true` if a cancel/lapse for this bet should be suppressed
    /// because a replace operation is pending or the bet was already replaced.
    pub fn should_suppress_cancel(&self, client_order_id: &ClientOrderId, bet_id: &str) -> bool {
        if self.replaced_venue_order_ids.contains(bet_id) {
            return true;
        }

        self.pending_replace_state
            .contains_key(&(*client_order_id, bet_id.to_string()))
    }

    pub(crate) fn is_retained_terminal_order(&self, client_order_id: &ClientOrderId) -> bool {
        self.order_correlations
            .get(client_order_id)
            .is_some_and(|correlation| correlation.terminal_retained)
    }

    pub(crate) fn should_suppress_replaced_report(&self, bet_id: &str) -> bool {
        if !self.replaced_venue_order_ids.contains(bet_id) {
            return false;
        }

        self.client_order_id_by_venue_order_id(bet_id)
            .is_none_or(|client_order_id| !self.is_retained_terminal_order(&client_order_id))
    }

    pub(crate) fn register_pending_replace(
        &mut self,
        client_order_id: ClientOrderId,
        old_bet_id: String,
        total_quantity: Option<Quantity>,
    ) {
        self.remove_terminal_retention(&client_order_id);
        let key = (client_order_id, old_bet_id);

        self.pending_replace_state.insert(
            key,
            PendingReplaceState {
                total_quantity,
                awaiting_reconciliation: false,
            },
        );
    }

    pub(crate) fn mark_pending_replace_ambiguous(
        &mut self,
        client_order_id: ClientOrderId,
        old_bet_id: &str,
    ) {
        let key = (client_order_id, old_bet_id.to_string());

        if let Some(pending) = self.pending_replace_state.get_mut(&key) {
            pending.awaiting_reconciliation = true;
        }
    }

    pub(crate) fn pending_replace_awaits_reconciliation(
        &self,
        client_order_id: &ClientOrderId,
        old_bet_id: &str,
    ) -> bool {
        self.pending_replace_state
            .get(&(*client_order_id, old_bet_id.to_string()))
            .is_some_and(|pending| pending.awaiting_reconciliation)
    }

    pub(crate) fn take_pending_replace(
        &mut self,
        client_order_id: ClientOrderId,
        old_bet_id: &str,
    ) -> Option<PendingReplaceState> {
        let key = (client_order_id, old_bet_id.to_string());
        self.pending_replace_state.remove(&key)
    }

    fn has_pending_replace(&self, client_order_id: &ClientOrderId) -> bool {
        self.pending_replace_state
            .keys()
            .any(|(candidate, _)| candidate == client_order_id)
    }

    pub(crate) fn mark_canceled_replace(&mut self, client_order_id: ClientOrderId, bet_id: &str) {
        self.canceled_replace_bet_ids.insert(bet_id.to_string());
        self.retain_terminal_order(client_order_id, bet_id);
    }

    pub(crate) fn is_canceled_replace(&self, bet_id: &str) -> bool {
        self.canceled_replace_bet_ids.contains(bet_id)
    }

    pub(crate) fn is_redundant_terminal_update(&self, order: &UnmatchedOrder) -> bool {
        self.terminal_orders.contains(&order.id)
            && !self.is_canceled_replace(&order.id)
            && !self.fill_tracker.has_unseen_fill(order)
            && !self.fill_tracker.has_unseen_fill_void(order)
    }

    pub(crate) fn clear_canceled_replace(&mut self, bet_id: &str) {
        self.canceled_replace_bet_ids.remove(bet_id);
    }

    /// Promotes a new bet observed while a price replacement is pending.
    ///
    /// Returns the total order quantity and old Bet ID when `new_bet_id` differs from one pending
    /// old Bet ID.
    pub(crate) fn promote_pending_replace(
        &mut self,
        client_order_id: &ClientOrderId,
        new_bet_id: &str,
        replacement_quantity: Quantity,
    ) -> Option<(Quantity, String)> {
        let mut old_bet_ids = self
            .pending_replace_state
            .keys()
            .filter(|(candidate, old_bet_id)| {
                candidate == client_order_id && old_bet_id != new_bet_id
            })
            .map(|(_, old_bet_id)| old_bet_id.clone());

        let old_bet_id = old_bet_ids.next()?;
        if old_bet_ids.next().is_some() {
            return None;
        }

        let pending = self.complete_pending_replace(
            *client_order_id,
            &old_bet_id,
            VenueOrderId::from(new_bet_id),
        )?;

        Some((
            pending.total_quantity.unwrap_or(replacement_quantity),
            old_bet_id,
        ))
    }

    pub(crate) fn complete_pending_replace(
        &mut self,
        client_order_id: ClientOrderId,
        old_bet_id: &str,
        new_venue_order_id: VenueOrderId,
    ) -> Option<PendingReplaceState> {
        if self
            .replaced_venue_order_ids
            .contains(new_venue_order_id.as_str())
        {
            return None;
        }

        let pending = self.take_pending_replace(client_order_id, old_bet_id)?;
        self.mark_replaced_venue_order_id(client_order_id, old_bet_id.to_string());
        self.bind_venue_order_id(&client_order_id, new_venue_order_id);
        Some(pending)
    }

    fn mark_replaced_venue_order_id(&mut self, client_order_id: ClientOrderId, bet_id: String) {
        self.mark_correlated_terminal_order(client_order_id, &bet_id);
        self.replaced_venue_order_ids.insert(bet_id);
    }

    pub(crate) fn register_pending_reduction(
        &mut self,
        client_order_id: ClientOrderId,
        bet_id: String,
        original_quantity: Quantity,
        requested_quantity: Quantity,
    ) {
        self.pending_reductions.insert(
            bet_id,
            PendingReductionState {
                client_order_id,
                original_quantity,
                requested_quantity,
                confirmed_quantity: None,
            },
        );
    }

    pub(crate) fn confirm_pending_reduction(
        &mut self,
        client_order_id: &ClientOrderId,
        bet_id: &str,
        active_quantity: Quantity,
    ) -> Option<Quantity> {
        let pending = self.pending_reductions.get_mut(bet_id)?;

        if !pending.can_confirm(client_order_id, active_quantity) {
            return None;
        }

        pending.confirmed_quantity = Some(active_quantity);
        Some(active_quantity)
    }

    pub(crate) fn complete_pending_reduction(
        &mut self,
        client_order_id: &ClientOrderId,
        bet_id: &str,
        quantity: Quantity,
    ) -> bool {
        let Some(pending) = self.pending_reductions.get_mut(bet_id) else {
            return false;
        };

        if !pending.is_unconfirmed_for(client_order_id) {
            return false;
        }

        pending.confirmed_quantity = Some(quantity);
        true
    }

    pub(crate) fn reduced_quantity(&self, bet_id: &str) -> Option<Quantity> {
        self.pending_reductions
            .get(bet_id)
            .and_then(|pending| pending.confirmed_quantity)
    }

    pub(crate) fn clear_pending_reduction(
        &mut self,
        client_order_id: &ClientOrderId,
        bet_id: &str,
    ) {
        if self
            .pending_reductions
            .get(bet_id)
            .is_some_and(|pending| pending.client_order_id == *client_order_id)
        {
            self.pending_reductions.remove(bet_id);
        }
    }

    /// Retains a locally owned terminal identity and all of its Bet ID state.
    pub(crate) fn retain_terminal_order(&mut self, client_order_id: ClientOrderId, bet_id: &str) {
        if self.replaced_venue_order_ids.contains(bet_id)
            || self.has_pending_replace(&client_order_id)
        {
            self.mark_correlated_terminal_order(client_order_id, bet_id);
            return;
        }

        self.bind_venue_order_id(&client_order_id, VenueOrderId::from(bet_id));
        self.mark_correlated_terminal_order(client_order_id, bet_id);

        self.retain_terminal_identity(client_order_id);
    }

    fn mark_correlated_terminal_order(&mut self, client_order_id: ClientOrderId, bet_id: &str) {
        if self
            .pending_reductions
            .get(bet_id)
            .is_some_and(|pending| pending.is_unconfirmed_for(&client_order_id))
        {
            self.pending_reductions.remove(bet_id);
        }

        let newly_correlated = self
            .order_correlations
            .entry(client_order_id)
            .or_default()
            .venue_order_ids
            .insert(bet_id.to_string());
        if newly_correlated {
            self.remove_external_terminal_retention(bet_id);
        }
        self.terminal_orders.insert(bet_id.to_string());
    }

    fn retain_terminal_identity(&mut self, client_order_id: ClientOrderId) {
        let correlation = self.order_correlations.entry(client_order_id).or_default();
        if correlation.terminal_retained {
            return;
        }

        correlation.terminal_retained = true;
        self.push_terminal_identity(TerminalRetentionKey::Owned(client_order_id));
    }

    /// Records an external terminal bet and bounds its stream and REST dedup state.
    pub fn mark_terminal_order(&mut self, bet_id: String) {
        if !self.terminal_orders.insert(bet_id.clone()) {
            return;
        }

        self.push_terminal_identity(TerminalRetentionKey::External(bet_id));
    }

    fn push_terminal_identity(&mut self, key: TerminalRetentionKey) {
        self.terminal_order_queue.push_back(key);
        if self.terminal_order_queue.len() > Self::DEDUP_RETENTION
            && let Some(expired) = self.terminal_order_queue.pop_front()
        {
            self.evict_terminal_identity(expired);
        }
    }

    fn remove_terminal_retention(&mut self, client_order_id: &ClientOrderId) {
        if self
            .order_correlations
            .get_mut(client_order_id)
            .is_some_and(|correlation| std::mem::take(&mut correlation.terminal_retained))
        {
            self.terminal_order_queue.retain(
                |key| !matches!(key, TerminalRetentionKey::Owned(candidate) if candidate == client_order_id),
            );
        }
    }

    fn remove_external_terminal_retention(&mut self, bet_id: &str) {
        if !self.terminal_orders.contains(bet_id) {
            return;
        }

        self.terminal_order_queue.retain(
            |key| !matches!(key, TerminalRetentionKey::External(candidate) if candidate == bet_id),
        );
    }

    pub(crate) fn mark_order_active(&mut self, client_order_id: &ClientOrderId, bet_id: &str) {
        self.remove_terminal_retention(client_order_id);
        self.terminal_orders.remove(bet_id);
        self.canceled_replace_bet_ids.remove(bet_id);
    }

    fn evict_terminal_identity(&mut self, key: TerminalRetentionKey) {
        match key {
            TerminalRetentionKey::Owned(client_order_id) => {
                let retained = self
                    .order_correlations
                    .get_mut(&client_order_id)
                    .is_some_and(|correlation| std::mem::take(&mut correlation.terminal_retained));
                if retained {
                    self.remove_order_correlation(&client_order_id);
                }
            }
            TerminalRetentionKey::External(bet_id) => {
                self.remove_venue_order_state(&bet_id);
            }
        }
    }

    fn remove_venue_order_state(&mut self, bet_id: &str) {
        self.terminal_orders.remove(bet_id);
        self.replaced_venue_order_ids.remove(bet_id);
        self.canceled_replace_bet_ids.remove(bet_id);
        self.pending_reductions.remove(bet_id);
        self.fill_tracker.prune(bet_id);
    }

    /// Anchors the fill tracker against cached orders so the post-reconnect
    /// image neither treats cumulative size as a new fill nor re-emits a
    /// fill that was published via another channel.
    pub fn sync_from_orders(&mut self, orders: &[OrderSyncEntry]) {
        for entry in orders {
            self.restore_order(
                entry.client_order_id,
                entry.strategy_id,
                VenueOrderId::from(entry.bet_id.as_str()),
            );

            for venue_order_id in &entry.venue_order_ids {
                if venue_order_id != &entry.bet_id {
                    self.mark_replaced_venue_order_id(
                        entry.client_order_id,
                        venue_order_id.clone(),
                    );
                }
            }

            if entry.is_closed {
                self.retain_terminal_order(entry.client_order_id, &entry.bet_id);
            } else {
                self.mark_order_active(&entry.client_order_id, &entry.bet_id);
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
    fn owned_terminal_retention_evicts_all_identity_state_together() {
        let mut state = OcmState::default();
        let client_order_id = ClientOrderId::from("O-TERMINAL-0");
        let strategy_id = StrategyId::from("S-001");
        let current_bet_id = "current-bet-0";
        let replaced_bet_id = "replaced-bet-0";
        let size_matched = Decimal::from(2);
        let average_price = Decimal::from(3);
        state.restore_order(
            client_order_id,
            strategy_id,
            VenueOrderId::from(current_bet_id),
        );
        state.mark_replaced_venue_order_id(client_order_id, replaced_bet_id.to_string());
        assert!(state.is_accepted(&client_order_id));
        state.register_pending_reduction(
            client_order_id,
            current_bet_id.to_string(),
            Quantity::from(10),
            Quantity::from(4),
        );
        state.confirm_pending_reduction(&client_order_id, current_bet_id, Quantity::from(4));
        state.fill_tracker.advance_cumulative_fill(
            current_bet_id,
            size_matched,
            Some(average_price),
            average_price,
        );
        state.fill_tracker.advance_cumulative_fill(
            replaced_bet_id,
            size_matched,
            Some(average_price),
            average_price,
        );
        state.retain_terminal_order(client_order_id, current_bet_id);

        for index in 0..OcmState::DEDUP_RETENTION {
            state.mark_terminal_order(format!("external-bet-{index}"));
        }

        let current_replay = state.fill_tracker.advance_cumulative_fill(
            current_bet_id,
            size_matched,
            Some(average_price),
            average_price,
        );
        let replaced_replay = state.fill_tracker.advance_cumulative_fill(
            replaced_bet_id,
            size_matched,
            Some(average_price),
            average_price,
        );
        let customer_order_ref = make_customer_order_ref(client_order_id.as_str());

        assert_eq!(state.order_strategy_id(&client_order_id), None);
        assert_eq!(
            state.resolve_client_order_id(Some(&customer_order_ref)),
            None,
        );
        assert_eq!(state.reduced_quantity(current_bet_id), None);
        assert!(!state.terminal_orders.contains(current_bet_id));
        assert!(!state.terminal_orders.contains(replaced_bet_id));
        assert!(!state.replaced_venue_order_ids.contains(replaced_bet_id));
        assert!(current_replay.is_some());
        assert!(replaced_replay.is_some());
    }

    #[rstest]
    fn terminal_retention_does_not_evict_active_or_ambiguous_replace_identity() {
        let mut state = OcmState::default();
        let active_client_order_id = ClientOrderId::from("O-ACTIVE");
        let ambiguous_client_order_id = ClientOrderId::from("O-AMBIGUOUS");
        let strategy_id = StrategyId::from("S-001");
        state.restore_order(
            active_client_order_id,
            strategy_id,
            VenueOrderId::from("active-bet"),
        );
        state.restore_order(
            ambiguous_client_order_id,
            strategy_id,
            VenueOrderId::from("ambiguous-bet"),
        );
        state.register_pending_replace(
            ambiguous_client_order_id,
            "ambiguous-bet".to_string(),
            Some(Quantity::from(10)),
        );
        state.mark_pending_replace_ambiguous(ambiguous_client_order_id, "ambiguous-bet");
        state.retain_terminal_order(ambiguous_client_order_id, "ambiguous-bet");

        for index in 0..=OcmState::DEDUP_RETENTION {
            state.mark_terminal_order(format!("external-bet-{index}"));
        }

        assert_eq!(
            state.order_strategy_id(&active_client_order_id),
            Some(strategy_id),
        );
        assert!(!state.mark_accepted(active_client_order_id));
        assert_eq!(
            state.order_strategy_id(&ambiguous_client_order_id),
            Some(strategy_id),
        );
        assert!(state.pending_replace_awaits_reconciliation(
            &ambiguous_client_order_id,
            "ambiguous-bet",
        ));
        assert!(state.should_suppress_cancel(&ambiguous_client_order_id, "ambiguous-bet",));
    }

    #[rstest]
    fn successful_replace_without_old_leg_ocm_uses_terminal_identity_lifecycle() {
        let mut state = OcmState::default();
        let client_order_id = ClientOrderId::from("O-REPLACE");
        let strategy_id = StrategyId::from("S-001");
        state.restore_order(client_order_id, strategy_id, VenueOrderId::from("old-bet"));
        state.register_pending_replace(
            client_order_id,
            "old-bet".to_string(),
            Some(Quantity::from(10)),
        );

        let completed = state.complete_pending_replace(
            client_order_id,
            "old-bet",
            VenueOrderId::from("new-bet"),
        );

        assert!(completed.is_some());
        assert!(state.pending_replace_state.is_empty());
        assert!(state.replaced_venue_order_ids.contains("old-bet"));
        assert!(state.terminal_orders.contains("old-bet"));
        assert_eq!(
            state.client_order_id_by_venue_order_id("old-bet"),
            Some(client_order_id),
        );
        assert_eq!(
            state.client_order_id_by_venue_order_id("new-bet"),
            Some(client_order_id),
        );
        assert!(!state.is_retained_terminal_order(&client_order_id));

        state.retain_terminal_order(client_order_id, "old-bet");

        assert_eq!(
            state
                .order_correlations
                .get(&client_order_id)
                .and_then(|correlation| correlation.venue_order_id),
            Some(VenueOrderId::from("new-bet")),
        );
        assert!(!state.is_retained_terminal_order(&client_order_id));

        state.retain_terminal_order(client_order_id, "new-bet");
        state.retain_terminal_order(client_order_id, "old-bet");

        assert_eq!(
            state
                .order_correlations
                .get(&client_order_id)
                .and_then(|correlation| correlation.venue_order_id),
            Some(VenueOrderId::from("new-bet")),
        );
        assert_eq!(state.terminal_order_queue.len(), 1);
    }

    #[rstest]
    fn replace_resolution_does_not_discard_another_pending_mutation() {
        let mut state = OcmState::default();
        let client_order_id = ClientOrderId::from("O-REPLACE");
        state.restore_order(
            client_order_id,
            StrategyId::from("S-001"),
            VenueOrderId::from("old-bet-1"),
        );
        state.register_pending_replace(
            client_order_id,
            "old-bet-1".to_string(),
            Some(Quantity::from(10)),
        );
        state.register_pending_replace(
            client_order_id,
            "old-bet-2".to_string(),
            Some(Quantity::from(10)),
        );

        let unresolved_promotion =
            state.promote_pending_replace(&client_order_id, "new-bet", Quantity::from(10));
        let completed = state.complete_pending_replace(
            client_order_id,
            "old-bet-1",
            VenueOrderId::from("new-bet"),
        );

        assert_eq!(unresolved_promotion, None);
        assert!(completed.is_some());
        assert!(
            !state
                .pending_replace_state
                .contains_key(&(client_order_id, "old-bet-1".to_string()))
        );
        assert!(
            state
                .pending_replace_state
                .contains_key(&(client_order_id, "old-bet-2".to_string()))
        );
        assert!(state.replaced_venue_order_ids.contains("old-bet-1"));
        assert!(!state.replaced_venue_order_ids.contains("old-bet-2"));
        assert_eq!(
            state.client_order_id_by_venue_order_id("new-bet"),
            Some(client_order_id),
        );
    }

    #[rstest]
    fn historical_bet_migrates_from_external_to_owned_identity() {
        let mut state = OcmState::default();
        let client_order_id = ClientOrderId::from("O-MIGRATED");
        let old_bet_id = "old-bet";
        let size_matched = Decimal::from(2);
        let average_price = Decimal::from(3);
        state.mark_terminal_order(old_bet_id.to_string());
        state.fill_tracker.advance_cumulative_fill(
            old_bet_id,
            size_matched,
            Some(average_price),
            average_price,
        );
        state.restore_order(
            client_order_id,
            StrategyId::from("S-001"),
            VenueOrderId::from("current-bet"),
        );
        state.mark_replaced_venue_order_id(client_order_id, old_bet_id.to_string());

        for index in 0..=OcmState::DEDUP_RETENTION {
            state.mark_terminal_order(format!("external-bet-{index}"));
        }

        let replay = state.fill_tracker.advance_cumulative_fill(
            old_bet_id,
            size_matched,
            Some(average_price),
            average_price,
        );

        assert_eq!(
            state.client_order_id_by_venue_order_id(old_bet_id),
            Some(client_order_id),
        );
        assert!(state.terminal_orders.contains(old_bet_id));
        assert!(state.replaced_venue_order_ids.contains(old_bet_id));
        assert!(replay.is_none());
    }

    #[rstest]
    fn sustained_owned_terminal_history_stays_bounded() {
        let mut state = OcmState::default();
        let strategy_id = StrategyId::from("S-001");

        for index in 0..=OcmState::DEDUP_RETENTION {
            let client_order_id = ClientOrderId::from(format!("O-{index}"));
            let current_bet_id = format!("current-bet-{index}");
            state.restore_order(
                client_order_id,
                strategy_id,
                VenueOrderId::from(current_bet_id.as_str()),
            );
            state.mark_replaced_venue_order_id(client_order_id, format!("replaced-bet-{index}"));
            state.retain_terminal_order(client_order_id, &current_bet_id);
        }

        assert_eq!(state.order_correlations.len(), OcmState::DEDUP_RETENTION);
        assert_eq!(
            state
                .order_correlations
                .values()
                .filter(|correlation| correlation.terminal_retained)
                .count(),
            OcmState::DEDUP_RETENTION,
        );
        assert_eq!(state.terminal_order_queue.len(), OcmState::DEDUP_RETENTION);
        assert_eq!(
            state.replaced_venue_order_ids.len(),
            OcmState::DEDUP_RETENTION,
        );
        assert_eq!(state.terminal_orders.len(), OcmState::DEDUP_RETENTION * 2);
        assert_eq!(state.order_strategy_id(&ClientOrderId::from("O-0")), None,);
        assert_eq!(
            state.order_strategy_id(&ClientOrderId::from(format!(
                "O-{}",
                OcmState::DEDUP_RETENTION
            ))),
            Some(strategy_id),
        );
    }

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
    fn active_submission_collision_is_rejected() {
        let suffix = "12345678901234567890123456789012";
        let first = ClientOrderId::from(format!("FIRST-{suffix}"));
        let second = ClientOrderId::from(format!("SECOND-{suffix}"));
        let strategy_id = StrategyId::from("S-001");
        let mut state = OcmState::default();

        state.register_submission(first, strategy_id).unwrap();
        let collision = state.register_submission(second, strategy_id);

        assert_eq!(collision, Err(suffix.to_string()));
        assert_eq!(state.resolve_client_order_id(Some(suffix)), Some(first));
        assert_eq!(state.order_strategy_id(&second), None);
    }

    #[rstest]
    fn submission_collision_with_restored_legacy_reference_is_rejected() {
        let reference = "12345678901234567890123456789012";
        let restored = ClientOrderId::from(format!("{reference}-RESTORED"));
        let fresh = ClientOrderId::from(format!("FRESH-{reference}"));
        let strategy_id = StrategyId::from("S-001");
        let mut state = OcmState::default();

        state.restore_order(restored, strategy_id, VenueOrderId::from("bet-restored"));
        let collision = state.register_submission(fresh, strategy_id);

        assert_eq!(collision, Err(reference.to_string()));
        assert_eq!(
            state.resolve_client_order_id(Some(reference)),
            Some(restored),
        );
        assert_eq!(state.order_strategy_id(&fresh), None);
    }

    #[rstest]
    fn restored_current_reference_ambiguity_recovers_after_cleanup() {
        let suffix = "12345678901234567890123456789012";
        let first = ClientOrderId::from(format!("FIRST-{suffix}"));
        let second = ClientOrderId::from(format!("SECOND-{suffix}"));
        let strategy_id = StrategyId::from("S-001");
        let mut state = OcmState::default();

        state.restore_order(first, strategy_id, VenueOrderId::from("bet-1"));
        state.restore_order(second, strategy_id, VenueOrderId::from("bet-2"));

        assert_eq!(
            state.customer_order_ref_resolution(suffix),
            Some(CustomerOrderRefResolution::Ambiguous),
        );
        assert_eq!(state.resolve_client_order_id(Some(suffix)), None);
        state.remove_order_correlation(&first);
        assert_eq!(state.resolve_client_order_id(Some(suffix)), Some(second));
        assert_eq!(
            state
                .order_correlations
                .get(&second)
                .and_then(|correlation| correlation.venue_order_id),
            Some(VenueOrderId::from("bet-2")),
        );
        assert!(!state.mark_accepted(second));
    }

    #[rstest]
    fn restored_legacy_reference_ambiguity_recovers_after_cleanup() {
        let prefix = "12345678901234567890123456789012";
        let first = ClientOrderId::from(format!("{prefix}-FIRST"));
        let second = ClientOrderId::from(format!("{prefix}-SECOND"));
        let strategy_id = StrategyId::from("S-001");
        let mut state = OcmState::default();

        state.restore_order(first, strategy_id, VenueOrderId::from("bet-1"));
        state.restore_order(second, strategy_id, VenueOrderId::from("bet-2"));

        assert_eq!(
            state.customer_order_ref_resolution(prefix),
            Some(CustomerOrderRefResolution::Ambiguous),
        );
        assert_eq!(state.resolve_client_order_id(Some(prefix)), None);
        state.remove_order_correlation(&second);
        assert_eq!(state.resolve_client_order_id(Some(prefix)), Some(first));
    }

    #[rstest]
    fn venue_identity_can_be_bound_and_replaced() {
        let client_order_id = ClientOrderId::from("O-1");
        let mut state = OcmState::default();
        state
            .register_submission(client_order_id, StrategyId::from("S-001"))
            .unwrap();

        state.bind_venue_order_id(&client_order_id, VenueOrderId::from("bet-1"));
        assert_eq!(
            state
                .order_correlations
                .get(&client_order_id)
                .and_then(|correlation| correlation.venue_order_id),
            Some(VenueOrderId::from("bet-1")),
        );

        state.bind_venue_order_id(&client_order_id, VenueOrderId::from("bet-2"));
        assert_eq!(
            state
                .order_correlations
                .get(&client_order_id)
                .and_then(|correlation| correlation.venue_order_id),
            Some(VenueOrderId::from("bet-2")),
        );
    }

    #[rstest]
    fn claimed_acceptance_does_not_replace_restored_venue_identity() {
        let client_order_id = ClientOrderId::from("O-1");
        let mut state = OcmState::default();
        state.restore_order(
            client_order_id,
            StrategyId::from("S-001"),
            VenueOrderId::from("bet-current"),
        );

        let claimed = state.claim_acceptance(client_order_id, VenueOrderId::from("bet-stale"));

        assert!(!claimed);
        assert_eq!(
            state
                .order_correlations
                .get(&client_order_id)
                .and_then(|correlation| correlation.venue_order_id),
            Some(VenueOrderId::from("bet-current")),
        );
    }

    #[rstest]
    fn pending_replace_promotes_only_a_different_bet() {
        let client_order_id = ClientOrderId::from("O-1");
        let mut state = OcmState::default();
        state.restore_order(
            client_order_id,
            StrategyId::from("S-001"),
            VenueOrderId::from("old-bet"),
        );
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
            Some((Quantity::from(10), "old-bet".to_string())),
        );
        assert!(state.pending_replace_state.is_empty());
        assert!(state.replaced_venue_order_ids.contains("old-bet"));
        assert_eq!(
            state
                .order_correlations
                .get(&client_order_id)
                .and_then(|correlation| correlation.venue_order_id),
            Some(VenueOrderId::from("new-bet")),
        );
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
            Some((Quantity::from(10), "current-bet".to_string())),
        );
    }

    #[rstest]
    #[case::below_requested(Quantity::from(3), None)]
    #[case::at_requested(Quantity::from(4), Some(Quantity::from(4)))]
    #[case::inside_window(Quantity::from(9), Some(Quantity::from(9)))]
    #[case::at_original(Quantity::from(10), None)]
    fn pending_reduction_confirms_only_a_definitive_reduction(
        #[case] active_quantity: Quantity,
        #[case] expected: Option<Quantity>,
    ) {
        let client_order_id = ClientOrderId::from("O-1");
        let bet_id = "bet-1";
        let mut state = OcmState::default();
        state.register_pending_reduction(
            client_order_id,
            bet_id.to_string(),
            Quantity::from(10),
            Quantity::from(4),
        );

        assert_eq!(
            state.confirm_pending_reduction(&client_order_id, bet_id, active_quantity),
            expected,
        );
        assert_eq!(state.reduced_quantity(bet_id), expected);
    }

    #[rstest]
    fn pending_reduction_validates_identity_and_resolves_once() {
        let client_order_id = ClientOrderId::from("O-1");
        let other_client_order_id = ClientOrderId::from("O-2");
        let bet_id = "bet-1";
        let mut state = OcmState::default();
        state.register_pending_reduction(
            client_order_id,
            bet_id.to_string(),
            Quantity::from(10),
            Quantity::from(4),
        );

        let mismatched =
            state.confirm_pending_reduction(&other_client_order_id, bet_id, Quantity::from(4));
        assert_eq!(mismatched, None);
        assert!(!state.complete_pending_reduction(
            &other_client_order_id,
            bet_id,
            Quantity::from(4),
        ));
        state.clear_pending_reduction(&other_client_order_id, bet_id);

        assert_eq!(
            state.confirm_pending_reduction(&client_order_id, bet_id, Quantity::from(4)),
            Some(Quantity::from(4)),
        );
        assert_eq!(
            state.confirm_pending_reduction(&client_order_id, bet_id, Quantity::from(4)),
            None,
            "a confirmed reduction must not resolve twice",
        );

        state.clear_pending_reduction(&client_order_id, bet_id);

        assert_eq!(state.reduced_quantity(bet_id), None);
        assert_eq!(
            state.confirm_pending_reduction(&client_order_id, bet_id, Quantity::from(4)),
            None,
            "a discarded reduction must not resolve from a later observation",
        );
    }

    #[rstest]
    fn terminal_order_retention_evicts_pending_reduction_state() {
        let client_order_id = ClientOrderId::from("O-1");
        let bet_id = "bet-0";
        let mut state = OcmState::default();
        state.register_pending_reduction(
            client_order_id,
            bet_id.to_string(),
            Quantity::from(10),
            Quantity::from(4),
        );
        state.confirm_pending_reduction(&client_order_id, bet_id, Quantity::from(4));

        for index in 0..=OcmState::DEDUP_RETENTION {
            state.mark_terminal_order(format!("bet-{index}"));
        }

        assert_eq!(state.reduced_quantity(bet_id), None);
    }

    #[rstest]
    fn canceled_replace_keeps_one_terminal_retention_entry() {
        let mut state = OcmState::default();
        let client_order_id = ClientOrderId::from("O-1");

        state.mark_canceled_replace(client_order_id, "old-bet");
        state.retain_terminal_order(client_order_id, "old-bet");

        assert!(state.terminal_orders.contains("old-bet"));
        assert_eq!(state.terminal_order_queue.len(), 1);
    }

    #[rstest]
    fn canceled_replace_without_ocm_has_terminal_retention_entry() {
        let mut state = OcmState::default();
        let client_order_id = ClientOrderId::from("O-1");

        state.mark_canceled_replace(client_order_id, "old-bet");

        assert!(state.terminal_orders.contains("old-bet"));
        assert_eq!(state.terminal_order_queue.len(), 1);
    }
}
