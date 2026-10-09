// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software distributed under the
//  License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
//  either express or implied. See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! The process-local settlement registry for the Polymarket execution client.
//!
//! One synchronized record per venue trade tracks venue settlement separately from per-leg core
//! application. The registry is the sole adapter authority for fill creation and correction:
//! stream evidence and targeted terminal REST results transition records through the settled
//! state machine, observed core events resolve pending applications, and reconciliation report
//! generation is gated on unresolved evidence. Terminal, pending, and hard-faulted records are
//! retained for the lifetime of the initialized client; capacity exhaustion faults the client.
//!
//! Observed fills that carry no venue trade ID (maker fills rebuilt from reconciliation reports)
//! are held as unbound legs keyed by their deterministic leg trade ID, and move into their trade
//! record once venue evidence for that trade is admitted.

use std::fmt::Debug;

use ahash::{AHashMap, AHashSet};
use nautilus_common::cache::fifo::FifoCache;
use nautilus_core::UnixNanos;
use nautilus_execution::reconciliation::inferred_reconciliation_trade_ids;
use nautilus_model::{
    enums::LiquiditySide,
    events::{OrderEventAny, OrderFillVoided, OrderFilled},
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    orders::{Order, OrderAny},
    reports::FillReport,
    types::Money,
};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    admission::{AdmittedLeg, AdmittedTrade, TakerFeeBasis, TakerFeeBasisLookup},
    state::{
        LegApplication, MAX_SETTLEMENT_RECORDS, SettlementAction, SettlementLeg, SettlementRecord,
        SettlementState, UncertainOrder, UncertainOrderKind,
    },
};
use crate::common::enums::PolymarketTradeStatus;

/// Registry state guarded by one mutex.
///
/// `session_orders` holds the venue orders noted in the current stream session. It is bounded;
/// an evicted order only degrades its fills to REST-gated application. `open_orders` holds the
/// current venue order ID and instrument of each order the engine holds open.
#[derive(Debug)]
struct RegistryInner {
    live: bool,
    session: u64,
    session_orders: FifoCache<VenueOrderId, 100_000>,
    open_orders: AHashMap<ClientOrderId, (VenueOrderId, InstrumentId)>,
    records: AHashMap<String, SettlementRecord>,
    leg_trade_ids: AHashMap<TradeId, String>,
    unbound_legs: AHashMap<TradeId, SettlementLeg>,
    uncertain_orders: AHashMap<VenueOrderId, UncertainOrder>,
    hydrated: usize,
    account_refresh_requested: bool,
    client_fault: Option<String>,
}

impl Default for RegistryInner {
    fn default() -> Self {
        Self {
            // Report generation is allowed until connect starts hydration
            live: true,
            session: 0,
            session_orders: FifoCache::new(),
            open_orders: AHashMap::new(),
            records: AHashMap::new(),
            leg_trade_ids: AHashMap::new(),
            unbound_legs: AHashMap::new(),
            uncertain_orders: AHashMap::new(),
            hydrated: 0,
            account_refresh_requested: false,
            client_fault: None,
        }
    }
}

/// Adapter-owned, process-local settlement registry keyed by venue trade ID.
///
/// The registry wakes the resolution task whenever a trade enters quarantine or requests a
/// refresh, so targeted terminal REST resolution starts immediately.
pub(crate) struct SettlementRegistry {
    account_id: AccountId,
    inner: Mutex<RegistryInner>,
    resolution_wakeup: tokio::sync::Notify,
}

impl Debug for SettlementRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock();
        f.debug_struct(stringify!(SettlementRegistry))
            .field("account_id", &self.account_id)
            .field("live", &inner.live)
            .field("session", &inner.session)
            .field("records", &inner.records.len())
            .field("unbound_legs", &inner.unbound_legs.len())
            .field("uncertain_orders", &inner.uncertain_orders.len())
            .field("client_fault", &inner.client_fault)
            .finish()
    }
}

impl SettlementRegistry {
    pub(crate) fn new(account_id: AccountId) -> Self {
        Self {
            account_id,
            inner: Mutex::new(RegistryInner::default()),
            resolution_wakeup: tokio::sync::Notify::new(),
        }
    }

    /// Clears all state; used when the client is reset or torn down.
    pub(crate) fn clear(&self) {
        *self.inner.lock() = RegistryInner::default();
    }

    /// Starts a new uninterrupted stream session, ending provisional-application eligibility
    /// for trades and orders admitted under earlier sessions.
    ///
    /// Provisional trades admitted in an earlier session request a targeted terminal REST read,
    /// because the stream does not replay terminal updates missed while disconnected.
    pub(crate) fn begin_session(&self) {
        let mut inner = self.inner.lock();
        inner.session += 1;
        inner.session_orders.clear();

        let mut requested = 0usize;

        for record in inner.records.values_mut() {
            if record.settlement == SettlementState::Provisional
                && record.admitted_session != 0
                && record.hard_fault.is_none()
                && !record.refresh_requested
            {
                record.refresh_requested = true;
                requested += 1;
            }
        }

        if requested > 0 {
            log::info!(
                "Requesting targeted REST resolution for {requested} provisional Polymarket \
                 trade(s) from an earlier stream session"
            );
        }

        if requested > 0 || !inner.uncertain_orders.is_empty() {
            self.resolution_wakeup.notify_one();
        }
    }

    /// Marks the registry hydrating without discarding records.
    ///
    /// Connect uses this while cache reconstruction and observer installation run; report
    /// generation fails closed until [`Self::mark_live`].
    pub(crate) fn mark_hydrating(&self) {
        self.inner.lock().live = false;
    }

    /// Marks hydration complete.
    pub(crate) fn mark_live(&self) {
        self.inner.lock().live = true;
    }

    /// Records that the client is submitting `venue_order_id` in the current stream session.
    ///
    /// Callers note the expected venue order ID before the submit request is sent, so trades
    /// that arrive ahead of the submit response remain eligible for provisional application.
    pub(crate) fn note_order_submitted(&self, venue_order_id: VenueOrderId) {
        self.inner.lock().session_orders.add(venue_order_id);
    }

    /// Carries the current-session note from the expected venue order ID to the ID the venue
    /// assigned in its submit response, when the two differ.
    ///
    /// The adapter tracks the order under the venue-assigned ID, so eligibility follows it. An
    /// order submitted before a session change stays ineligible and awaits a targeted REST read,
    /// because the stream does not replay trades that matched while it was disconnected.
    pub(crate) fn note_order_accepted(
        &self,
        expected_venue_order_id: VenueOrderId,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        noted_at: UnixNanos,
    ) {
        let mut inner = self.inner.lock();

        if !inner.session_orders.contains(&expected_venue_order_id) {
            insert_stream_gap_order(
                &mut inner.uncertain_orders,
                venue_order_id,
                instrument_id,
                noted_at,
            );
            self.resolution_wakeup.notify_one();
        } else if venue_order_id != expected_venue_order_id {
            inner.session_orders.add(venue_order_id);
        }
    }

    /// Records that the engine holds `client_order_id` open under `venue_order_id`.
    pub(crate) fn note_order_open(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
    ) {
        self.inner
            .lock()
            .open_orders
            .insert(client_order_id, (venue_order_id, instrument_id));
    }

    /// Records that the engine no longer holds `client_order_id` open.
    pub(crate) fn note_order_closed(&self, client_order_id: &ClientOrderId) {
        self.inner.lock().open_orders.remove(client_order_id);
    }

    /// Requests a targeted REST read for every open order after the user stream reconnects.
    ///
    /// The stream does not replay trades that matched while it was disconnected, so reports
    /// touching these orders fail closed until the read establishes their trades.
    pub(crate) fn note_stream_gap(&self, noted_at: UnixNanos) {
        let mut guard = self.inner.lock();
        let inner = &mut *guard;

        for (venue_order_id, instrument_id) in inner.open_orders.values() {
            insert_stream_gap_order(
                &mut inner.uncertain_orders,
                *venue_order_id,
                *instrument_id,
                noted_at,
            );
        }

        if !inner.open_orders.is_empty() {
            log::info!(
                "Requesting targeted REST reads for {} open Polymarket order(s) after a user \
                 stream reconnect",
                inner.open_orders.len()
            );
            self.resolution_wakeup.notify_one();
        }
    }

    /// Records a submitted order whose venue outcome is unknown.
    ///
    /// Reports touching the order fail closed until a targeted REST read establishes its venue
    /// state, because stream updates missed during the outage are not replayed.
    pub(crate) fn note_order_uncertain(
        &self,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        noted_at: UnixNanos,
    ) {
        let uncertain = UncertainOrder {
            instrument_id,
            noted_at,
            kind: UncertainOrderKind::Submit,
        };

        self.inner
            .lock()
            .uncertain_orders
            .entry(venue_order_id)
            .and_modify(|existing| {
                if existing.kind != UncertainOrderKind::SubmitClosed {
                    *existing = uncertain;
                }
            })
            .or_insert(uncertain);

        self.resolution_wakeup.notify_one();
    }

    /// Retains terminal-submit recovery until venue closure and trade evidence are established.
    pub(crate) fn note_uncertain_submit_closed(&self, venue_order_id: &VenueOrderId) {
        if let Some(order) = self.inner.lock().uncertain_orders.get_mut(venue_order_id)
            && order.kind == UncertainOrderKind::Submit
        {
            order.kind = UncertainOrderKind::SubmitClosed;
        }
    }

    /// Returns the orders whose venue state awaits a targeted REST read.
    pub(crate) fn uncertain_orders(&self) -> Vec<(VenueOrderId, UncertainOrder)> {
        self.inner
            .lock()
            .uncertain_orders
            .iter()
            .map(|(venue_order_id, order)| (*venue_order_id, *order))
            .collect()
    }

    /// Ends tracking of an uncertain order once its venue state is applied.
    pub(crate) fn clear_uncertain_order(&self, venue_order_id: &VenueOrderId) {
        self.inner.lock().uncertain_orders.remove(venue_order_id);
    }

    /// Reconstructs an observed applied fill from retained core events during hydration.
    ///
    /// Presence is authoritative: the leg is recorded as applied.
    pub(crate) fn hydrate_fill(&self, fill: &OrderFilled) {
        record_observed_fill(&mut self.inner.lock(), fill);
    }

    /// Reconstructs an observed applied void from retained core events during hydration.
    pub(crate) fn hydrate_void(&self, voided: &OrderFillVoided) {
        record_observed_void(&mut self.inner.lock(), voided);
    }

    /// Transitions the registry with admitted stream evidence and returns the effects to apply.
    pub(crate) fn admit_stream_trade(&self, admitted: &AdmittedTrade) -> Vec<SettlementAction> {
        let mut inner = self.inner.lock();
        let key = admitted.venue_trade_id.as_str();
        let session = inner.session;
        if inner.client_fault.is_some() || !ensure_record(&mut inner, key, session) {
            return Vec::new();
        }

        bind_unbound_legs(&mut inner, key, &admitted.legs);
        let mut actions = Vec::new();
        stream_transition(&mut inner, key, admitted, &mut actions);
        self.wake_if_awaiting_resolution(&inner, key);
        actions
    }

    /// Transitions the registry with the result of a targeted terminal REST read.
    ///
    /// Non-terminal results leave the record unchanged; the resolution loop retries with capped
    /// backoff. The first admitted terminal result is final: a materially different later
    /// result hard-faults the trade.
    pub(crate) fn admit_rest_result(&self, admitted: &AdmittedTrade) -> Vec<SettlementAction> {
        let terminal_failed = admitted.status == PolymarketTradeStatus::Failed;
        if !terminal_failed && admitted.status != PolymarketTradeStatus::Confirmed {
            return Vec::new();
        }

        let mut inner = self.inner.lock();
        let key = admitted.venue_trade_id.as_str();
        if inner.client_fault.is_some() || !ensure_record(&mut inner, key, 0) {
            return Vec::new();
        }

        bind_unbound_legs(&mut inner, key, &admitted.legs);
        let mut actions = Vec::new();
        rest_transition(
            &mut inner,
            key,
            &admitted.legs,
            terminal_failed,
            &mut actions,
        );
        actions
    }

    /// Quarantines a trade whose stream evidence failed shared validation, so targeted REST
    /// resolution can supply clean evidence; a REST-settled trade requests a refresh instead.
    pub(crate) fn quarantine_invalid_trade(&self, venue_trade_id: &str) {
        let mut inner = self.inner.lock();
        if inner.client_fault.is_some() || !ensure_record(&mut inner, venue_trade_id, 0) {
            return;
        }

        self.quarantine_record(&mut inner, venue_trade_id);
    }

    /// Requests terminal REST resolution for contradictory unowned evidence of a known trade.
    /// Unknown unowned trades do not create settlement records.
    pub(crate) fn quarantine_known_trade(&self, venue_trade_id: &str) {
        let mut inner = self.inner.lock();
        if inner.client_fault.is_some() {
            return;
        }

        self.quarantine_record(&mut inner, venue_trade_id);
    }

    /// Marks the leg for an `OrderFilled` sent to core as pending application.
    pub(crate) fn note_leg_enqueued(&self, trade_id: &TradeId) {
        self.update_bound_leg(trade_id, |leg| {
            if leg.application == LegApplication::Absent {
                leg.application = LegApplication::FillPending;
                leg.authorized = false;
            }
        });
    }

    /// Marks the leg as delivered through a fill report for an order without captured context.
    pub(crate) fn note_leg_reported(&self, trade_id: &TradeId) {
        self.update_bound_leg(trade_id, |leg| {
            if leg.application == LegApplication::Absent {
                leg.authorized = false;
                leg.report_routed = true;
            }
        });
    }

    /// Decides whether a buffered fill may be emitted now that its order has registered.
    ///
    /// The fill emits only while its trade still permits application, and a provisional trade
    /// only within the stream session that admitted it. Otherwise its authorization is
    /// withdrawn, leaving the leg absent for targeted REST resolution or as a tombstone under a
    /// REST-established `FAILED` outcome.
    pub(crate) fn claim_buffered_fill(&self, venue_trade_id: &str, trade_id: &TradeId) -> bool {
        let mut inner = self.inner.lock();
        let faulted = inner.client_fault.is_some();
        let session = inner.session;

        let Some(record) = inner.records.get_mut(venue_trade_id) else {
            return false;
        };

        let permits = !faulted
            && record.permits_application()
            && (record.settlement != SettlementState::Provisional
                || record.admitted_session == session);

        let Some(leg) = record.leg_by_trade_id(trade_id) else {
            return false;
        };

        if !leg.authorized {
            return false;
        }

        if !permits {
            leg.authorized = false;
        }

        permits
    }

    /// Observes an applied `OrderFilled` published by core for this venue and account.
    ///
    /// Returns the venue trade ID and applied fill to void when the trade already settled
    /// `FAILED` through REST: a late matching core event still receives its one required void,
    /// and a later hard fault does not cancel that correction.
    pub(crate) fn observe_fill_applied(
        &self,
        fill: &OrderFilled,
    ) -> Option<(String, Box<OrderFilled>)> {
        let mut inner = self.inner.lock();
        if inner.client_fault.is_some() {
            return None;
        }

        record_observed_fill(&mut inner, fill);
        let key = inner.leg_trade_ids.get(&fill.trade_id).cloned()?;
        let record = inner.records.get_mut(&key)?;
        if record.settlement != SettlementState::RestFailed {
            return None;
        }

        let leg = record.leg_by_trade_id(&fill.trade_id)?;
        if leg.application != LegApplication::FillObserved {
            return None;
        }

        leg.application = LegApplication::VoidPending;
        Some((key, Box::new(fill.clone())))
    }

    /// Observes an applied `OrderFillVoided` published by core for this venue and account.
    pub(crate) fn observe_void_applied(&self, voided: &OrderFillVoided) {
        let mut inner = self.inner.lock();
        if inner.client_fault.is_none() {
            record_observed_void(&mut inner, voided);
        }
    }

    /// Observes a fill or void that core declined and republished on the fill-declined topic.
    ///
    /// A declined fill hard-faults its trade, because every leg must reach an explicit outcome
    /// and the registry applies a leg only once. The exception is a trade already settled
    /// `FAILED` through REST, where the absent leg is the correct tombstone. A declined
    /// correction void hard-faults the trade because the applied fill cannot be reversed.
    pub(crate) fn observe_fill_declined(&self, event: &OrderEventAny) {
        let (trade_id, pending, restored) = match event {
            OrderEventAny::Filled(fill) => (
                fill.trade_id,
                LegApplication::FillPending,
                LegApplication::Absent,
            ),
            OrderEventAny::FillVoided(voided) => (
                voided.trade_id,
                LegApplication::VoidPending,
                LegApplication::FillObserved,
            ),
            _ => return,
        };

        let mut inner = self.inner.lock();
        if inner.client_fault.is_some() {
            return;
        }

        let Some(key) = inner.leg_trade_ids.get(&trade_id).cloned() else {
            log::error!(
                "Declined Polymarket fill event with trade ID {trade_id} is not bound to a \
                 settlement record (engine logs carry the decline reason)"
            );
            return;
        };

        let Some(record) = inner.records.get_mut(&key) else {
            return;
        };

        let settlement = record.settlement;

        let Some(leg) = record.leg_by_trade_id(&trade_id) else {
            return;
        };

        // Any other state was already resolved by an observed event, which is authoritative
        if leg.application != pending {
            return;
        }

        leg.application = restored;

        let reason = match event {
            OrderEventAny::Filled(_) if settlement == SettlementState::RestFailed => return,
            OrderEventAny::Filled(_) => {
                format!("core declined the fill for trade {key} leg {trade_id}")
            }
            _ => format!(
                "core declined the correction void for trade {key} leg {trade_id}; the applied \
                 fill cannot be reversed"
            ),
        };

        hard_fault(&mut inner, &key, reason);
    }

    /// Returns trade IDs awaiting a targeted terminal REST read: quarantined trades, REST-settled
    /// trades whose terminal result was contradicted by later stream evidence, and provisional
    /// trades carried across a stream session change.
    pub(crate) fn pending_resolutions(&self) -> Vec<String> {
        self.inner
            .lock()
            .records
            .values()
            .filter(|record| record.awaits_resolution())
            .map(|record| record.venue_trade_id.clone())
            .collect()
    }

    /// Completes when a trade entered quarantine or requested a refresh since the last wakeup.
    pub(crate) async fn resolution_requested(&self) {
        self.resolution_wakeup.notified().await;
    }

    /// Returns and clears whether a terminal outcome or hard fault requested an account refresh.
    pub(crate) fn take_account_refresh(&self) -> bool {
        std::mem::take(&mut self.inner.lock().account_refresh_requested)
    }

    /// Returns whether the trade reached a confirmed settlement (stream or REST).
    pub(crate) fn is_trade_confirmed(&self, venue_trade_id: &str) -> bool {
        self.inner
            .lock()
            .records
            .get(venue_trade_id)
            .is_some_and(|record| {
                matches!(
                    record.settlement,
                    SettlementState::StreamConfirmed | SettlementState::RestConfirmed
                )
            })
    }

    /// Returns whether the registry holds evidence or an observed fill for the trade.
    pub(crate) fn knows_trade(&self, admitted: &AdmittedTrade) -> bool {
        let inner = self.inner.lock();
        inner.records.contains_key(&admitted.venue_trade_id)
            || admitted
                .legs
                .iter()
                .any(|leg| inner.unbound_legs.contains_key(&leg.trade_id))
    }

    /// Checks scoped confirmed report evidence without establishing a terminal trade outcome.
    ///
    /// Contradictory retained evidence fails the report closed; only a targeted complete-trade
    /// REST result may transition or hard-fault the trade.
    /// Retained leg economics take precedence over non-terminal REST copies.
    pub(crate) fn build_fill_report(
        &self,
        venue_trade_id: &str,
        incoming: &AdmittedLeg,
        ts_init: UnixNanos,
    ) -> anyhow::Result<FillReport> {
        let inner = self.inner.lock();
        anyhow::ensure!(
            inner.client_fault.is_none(),
            "settlement registry is faulted"
        );

        let stored = if let Some(record) = inner.records.get(venue_trade_id) {
            anyhow::ensure!(
                record.settlement != SettlementState::RestFailed,
                "report trade {venue_trade_id} contradicts retained settlement: settled FAILED",
            );
            anyhow::ensure!(
                !record.is_unresolved(),
                "report trade {venue_trade_id} has unresolved settlement evidence",
            );
            let stored = record
                .legs
                .iter()
                .find(|leg| leg.trade_id == incoming.trade_id);
            anyhow::ensure!(
                !record.settlement.is_rest_terminal() || stored.is_some(),
                "report trade {venue_trade_id} adds a leg to retained terminal settlement",
            );

            if let Some(stored) = stored {
                anyhow::ensure!(
                    !leg_evidence_conflicts(stored, incoming, record.settlement.is_rest_terminal()),
                    "report trade {venue_trade_id} contradicts retained leg {}",
                    incoming.trade_id,
                );
            }

            stored
        } else {
            None
        }
        .or_else(|| inner.unbound_legs.get(&incoming.trade_id));

        let mut report = incoming.fill_report(self.account_id, ts_init);

        if let Some(stored) = stored {
            anyhow::ensure!(
                !leg_evidence_conflicts(stored, incoming, false),
                "report trade {venue_trade_id} contradicts observed leg {}",
                incoming.trade_id,
            );

            if stored.application == LegApplication::FillObserved {
                report.last_qty = stored.last_qty;
                report.last_px = stored.last_px;
                report.commission = stored.commission.clone();
                report.ts_event = stored.ts_event;
            }
        }

        Ok(report)
    }

    /// Returns per-order quantities from effective core fills plus validated report legs.
    ///
    /// Venue fill IDs deduplicate reports; inferred fills provide a floor rather than additive
    /// evidence. Cumulative core void events remove only the corrected quantity.
    /// A remaining core fill from retained targeted REST FAILED evidence blocks the report.
    pub(crate) fn report_filled_quantities<'a>(
        &self,
        reports: &[FillReport],
        orders: impl IntoIterator<Item = &'a OrderAny>,
    ) -> anyhow::Result<AHashMap<VenueOrderId, Decimal>> {
        let inner = self.inner.lock();
        let mut quantities = AHashMap::<VenueOrderId, Decimal>::new();
        let mut floors = AHashMap::<VenueOrderId, Decimal>::new();
        let mut included = AHashSet::new();

        for order in orders
            .into_iter()
            .filter(|order| order.account_id() == Some(self.account_id))
        {
            for (fill, quantity, inferred) in order_report_fills(order)? {
                anyhow::ensure!(
                    quantity.is_zero()
                        || !inner
                            .leg_trade_ids
                            .get(&fill.trade_id)
                            .and_then(|key| inner.records.get(key))
                            .is_some_and(|record| record.settlement == SettlementState::RestFailed),
                    "core fill {} retains quantity after targeted REST FAILED settlement",
                    fill.trade_id,
                );
                *floors.entry(fill.venue_order_id).or_default() += quantity;
                if !inferred && included.insert(fill.trade_id) {
                    *quantities.entry(fill.venue_order_id).or_default() += quantity;
                }
            }
        }

        for report in reports {
            if included.insert(report.trade_id) {
                *quantities.entry(report.venue_order_id).or_default() +=
                    report.last_qty.as_decimal();
            }
        }

        for (venue_order_id, floor) in floors {
            let quantity = quantities.entry(venue_order_id).or_default();
            *quantity = (*quantity).max(floor);
        }

        Ok(quantities)
    }

    /// Fails report generation while the registry is hydrating or holds unresolved evidence in
    /// the requested instrument scope (or the whole account), so reconciliation cannot infer fill
    /// economics from incomplete coverage.
    ///
    /// A record without admitted legs has no known instrument, so it blocks every scope.
    pub(crate) fn ensure_resolved(
        &self,
        instrument_id: Option<InstrumentId>,
        report: &str,
    ) -> anyhow::Result<()> {
        self.ensure_no_blockers(
            report,
            |leg| instrument_id.is_none_or(|id| leg.instrument_id == id),
            |_, order| instrument_id.is_none_or(|id| order.instrument_id == id),
        )
    }

    /// Fails report generation while evidence touching one venue order is unresolved.
    pub(crate) fn ensure_order_resolved(
        &self,
        venue_order_id: &VenueOrderId,
        report: &str,
    ) -> anyhow::Result<()> {
        self.ensure_no_blockers(
            &format!("{report} for venue order {venue_order_id}"),
            |leg| leg.venue_order_id == *venue_order_id,
            |uncertain_order_id, _| uncertain_order_id == venue_order_id,
        )
    }

    pub(crate) fn client_faulted(&self) -> bool {
        self.inner.lock().client_fault.is_some()
    }

    pub(crate) fn client_fault_reason(&self) -> Option<String> {
        self.inner.lock().client_fault.clone()
    }

    pub(crate) fn record_count(&self) -> usize {
        self.inner.lock().records.len()
    }

    fn update_bound_leg(&self, trade_id: &TradeId, update: impl FnOnce(&mut SettlementLeg)) {
        let mut inner = self.inner.lock();

        let Some(key) = inner.leg_trade_ids.get(trade_id).cloned() else {
            return;
        };

        if let Some(leg) = inner
            .records
            .get_mut(&key)
            .and_then(|record| record.leg_by_trade_id(trade_id))
        {
            update(leg);
        }
    }

    fn ensure_no_blockers(
        &self,
        report: &str,
        in_scope: impl Fn(&SettlementLeg) -> bool,
        order_in_scope: impl Fn(&VenueOrderId, &UncertainOrder) -> bool,
    ) -> anyhow::Result<()> {
        let inner = self.inner.lock();
        anyhow::ensure!(
            inner.live,
            "cannot generate {report}: Polymarket settlement registry is hydrating"
        );

        let count = inner
            .records
            .values()
            .filter(|record| {
                (record.legs.is_empty() || record.legs.iter().any(&in_scope))
                    && record.is_unresolved()
            })
            .count();

        anyhow::ensure!(
            count == 0,
            "cannot generate {report}: Polymarket settlement registry holds {count} record(s) \
             with unresolved evidence"
        );

        let (submits, stream_gaps) = inner
            .uncertain_orders
            .iter()
            .filter(|(venue_order_id, order)| order_in_scope(venue_order_id, order))
            .fold((0, 0), |(submits, stream_gaps), (_, order)| {
                match order.kind {
                    UncertainOrderKind::Submit | UncertainOrderKind::SubmitClosed => {
                        (submits + 1, stream_gaps)
                    }
                    UncertainOrderKind::StreamGap => (submits, stream_gaps + 1),
                }
            });

        anyhow::ensure!(
            submits == 0,
            "cannot generate {report}: {submits} Polymarket order(s) have an unknown submit \
             outcome"
        );
        anyhow::ensure!(
            stream_gaps == 0,
            "cannot generate {report}: {stream_gaps} Polymarket order(s) await a trade read \
             after a user stream reconnect"
        );
        Ok(())
    }

    fn quarantine_record(&self, inner: &mut RegistryInner, venue_trade_id: &str) {
        if let Some(record) = inner.records.get_mut(venue_trade_id)
            && record.hard_fault.is_none()
        {
            if record.settlement.is_rest_terminal() {
                if !record.refresh_requested {
                    log::warn!(
                        "Requesting terminal REST refresh for Polymarket trade {venue_trade_id} \
                         after invalid or contradictory stream evidence"
                    );
                }

                record.refresh_requested = true;
            } else {
                enter_quarantined(record);
            }
        }

        self.wake_if_awaiting_resolution(inner, venue_trade_id);
    }

    fn wake_if_awaiting_resolution(&self, inner: &RegistryInner, key: &str) {
        if inner
            .records
            .get(key)
            .is_some_and(SettlementRecord::awaits_resolution)
        {
            self.resolution_wakeup.notify_one();
        }
    }
}

impl TakerFeeBasisLookup for SettlementRegistry {
    fn taker_fee_basis(
        &self,
        venue_trade_id: &str,
        venue_order_id: &VenueOrderId,
    ) -> Option<TakerFeeBasis> {
        let inner = self.inner.lock();
        inner.records.get(venue_trade_id).and_then(|record| {
            record
                .legs
                .iter()
                .find(|leg| &leg.venue_order_id == venue_order_id)
                .and_then(|leg| leg.taker_fee_basis)
        })
    }
}

fn stream_transition(
    inner: &mut RegistryInner,
    key: &str,
    admitted: &AdmittedTrade,
    actions: &mut Vec<SettlementAction>,
) {
    let Some(record) = inner.records.get(key) else {
        return;
    };

    let conflict = stream_evidence_conflicts(record, admitted);

    let new_legs: Vec<AdmittedLeg> = admitted
        .legs
        .iter()
        .filter(|incoming| {
            !record
                .legs
                .iter()
                .any(|leg| leg.venue_order_id == incoming.venue_order_id)
        })
        .cloned()
        .collect();

    let eligible = stream_application_eligible(inner, record, &new_legs);

    let Some(record) = inner.records.get_mut(key) else {
        return;
    };

    if record.hard_fault.is_some() {
        return;
    }

    let is_failed = admitted.status == PolymarketTradeStatus::Failed;

    match record.settlement {
        SettlementState::Provisional | SettlementState::StreamConfirmed => {
            let recorded = admit_provisional_evidence(
                record,
                admitted,
                &new_legs,
                is_failed || conflict,
                eligible,
                actions,
            );

            if recorded {
                for leg in &new_legs {
                    inner.leg_trade_ids.insert(leg.trade_id, key.to_string());
                }
            }
        }
        // Remain quarantined and continue targeted resolution
        SettlementState::Quarantined => {}
        // Matching evidence is a no-op. Contradictions refresh without reversal; under
        // `RestFailed` a newly observed fill receives its void through the observation path
        SettlementState::RestConfirmed | SettlementState::RestFailed => {
            let matches_outcome = is_failed == (record.settlement == SettlementState::RestFailed);
            if !matches_outcome || conflict || !new_legs.is_empty() {
                record.refresh_requested = true;
            }
        }
    }
}

/// Applies stream evidence to a record not yet settled through REST, returning whether the new
/// legs were recorded.
fn admit_provisional_evidence(
    record: &mut SettlementRecord,
    admitted: &AdmittedTrade,
    new_legs: &[AdmittedLeg],
    contradicts: bool,
    eligible: bool,
    actions: &mut Vec<SettlementAction>,
) -> bool {
    // A provisional status after confirmation never regresses it
    if admitted.status == PolymarketTradeStatus::Confirmed {
        record.settlement = SettlementState::StreamConfirmed;
    }

    if contradicts {
        enter_quarantined(record);
        return false;
    }

    record
        .legs
        .extend(new_legs.iter().map(SettlementLeg::from_admitted));

    if eligible {
        apply_absent_legs(record, admitted, actions);
    } else if record.legs.iter().any(SettlementLeg::awaits_application) {
        // Post-reconnect or restored-order evidence is REST-gated
        enter_quarantined(record);
    }

    true
}

fn rest_transition(
    inner: &mut RegistryInner,
    key: &str,
    admitted_legs: &[AdmittedLeg],
    terminal_failed: bool,
    actions: &mut Vec<SettlementAction>,
) {
    let Some(record) = inner.records.get_mut(key) else {
        return;
    };

    // A hard-faulted trade admits no new effects; a terminal result accepted before the fault
    // is retained
    if record.hard_fault.is_some() {
        return;
    }

    let outcome = if terminal_failed {
        "FAILED"
    } else {
        "CONFIRMED"
    };

    if let Some(previous) = &record.terminal_rest_legs {
        let matches = (record.settlement == SettlementState::RestFailed) == terminal_failed
            && legs_materially_equal(previous, admitted_legs);
        record.refresh_requested = false;

        if !matches {
            let reason = format!(
                "conflicting terminal REST result for trade {key}: fresh {outcome} evidence \
                 differs from the retained terminal result"
            );
            hard_fault(inner, key, reason);
        }

        return;
    }

    // Every leg with local application history must be covered by the authoritative result;
    // an uncovered applied or authorized leg is ambiguous local history
    let uncovered = record.legs.iter().any(|leg| {
        (leg.application != LegApplication::Absent || leg.authorized)
            && !admitted_legs
                .iter()
                .any(|incoming| incoming.venue_order_id == leg.venue_order_id)
    });

    if uncovered {
        let reason = format!(
            "terminal REST {outcome} evidence for trade {key} does not cover every locally \
             applied leg"
        );
        hard_fault(inner, key, reason);
        return;
    }

    if record.settlement == SettlementState::Quarantined || record.refresh_requested {
        log::info!("Resolved Polymarket trade {key} as {outcome} from targeted REST evidence");
    }

    record.terminal_rest_legs = Some(admitted_legs.to_vec());
    record.refresh_requested = false;
    inner.account_refresh_requested = true;

    if terminal_failed {
        enter_rest_failed(inner, key, admitted_legs, actions);
    } else {
        enter_rest_confirmed(inner, key, admitted_legs, actions);
    }
}

fn enter_rest_confirmed(
    inner: &mut RegistryInner,
    key: &str,
    admitted: &[AdmittedLeg],
    actions: &mut Vec<SettlementAction>,
) {
    let Some(record) = inner.records.get_mut(key) else {
        return;
    };

    record.settlement = SettlementState::RestConfirmed;

    let mut new_trade_ids = Vec::new();
    let mut divergent_order = None;

    for incoming in admitted {
        let Some(leg) = record
            .legs
            .iter_mut()
            .find(|leg| leg.venue_order_id == incoming.venue_order_id)
        else {
            let mut leg = SettlementLeg::from_admitted(incoming);
            leg.authorized = true;
            record.legs.push(leg);
            new_trade_ids.push(incoming.trade_id);
            actions.push(SettlementAction::ApplyLeg {
                venue_trade_id: key.to_string(),
                leg: incoming.clone(),
            });

            continue;
        };

        if leg.awaits_application() {
            // Terminal REST evidence is authoritative for a leg never sent to core
            inner.leg_trade_ids.remove(&leg.trade_id);
            inner
                .leg_trade_ids
                .insert(incoming.trade_id, key.to_string());
            *leg = SettlementLeg::from_admitted(incoming);
            leg.authorized = true;
            actions.push(SettlementAction::ApplyLeg {
                venue_trade_id: key.to_string(),
                leg: incoming.clone(),
            });
        } else if (leg.application != LegApplication::Absent || leg.authorized)
            && leg_evidence_conflicts(leg, incoming, true)
        {
            divergent_order = Some(leg.venue_order_id);
            break;
        }
    }

    for trade_id in new_trade_ids {
        inner.leg_trade_ids.insert(trade_id, key.to_string());
    }

    if let Some(venue_order_id) = divergent_order {
        actions.retain(|action| !matches!(action, SettlementAction::ApplyLeg { .. }));
        let reason = format!(
            "terminal REST CONFIRMED evidence for trade {key} contradicts the applied fill on \
             order {venue_order_id}"
        );
        hard_fault(inner, key, reason);
    }
}

fn enter_rest_failed(
    inner: &mut RegistryInner,
    key: &str,
    admitted: &[AdmittedLeg],
    actions: &mut Vec<SettlementAction>,
) {
    let Some(record) = inner.records.get_mut(key) else {
        return;
    };

    record.settlement = SettlementState::RestFailed;

    let mut new_trade_ids = Vec::new();

    for incoming in admitted {
        let Some(leg) = record
            .legs
            .iter_mut()
            .find(|leg| leg.venue_order_id == incoming.venue_order_id)
        else {
            // A leg absent from local state tombstones immediately
            record.legs.push(SettlementLeg::from_admitted(incoming));
            new_trade_ids.push(incoming.trade_id);
            continue;
        };

        // Absent legs tombstone (buffered copies are refused when they drain); pending fills
        // receive their void on observation
        if leg.application == LegApplication::FillObserved
            && let Some(fill) = leg.applied_fill.clone()
        {
            leg.application = LegApplication::VoidPending;
            actions.push(SettlementAction::VoidAppliedFill {
                venue_trade_id: key.to_string(),
                fill,
            });
        }
    }

    for trade_id in new_trade_ids {
        inner.leg_trade_ids.insert(trade_id, key.to_string());
    }
}

fn stream_evidence_conflicts(record: &SettlementRecord, admitted: &AdmittedTrade) -> bool {
    record.legs.iter().any(|stored| {
        if stored.report_routed || !stored.venue_evidence {
            return false;
        }

        match admitted
            .legs
            .iter()
            .find(|incoming| incoming.venue_order_id == stored.venue_order_id)
        {
            // A stored leg missing from a complete payload: the owned-leg set changed
            None => true,
            Some(incoming) => leg_evidence_conflicts(stored, incoming, admitted.has_economics()),
        }
    })
}

/// Compares incoming admitted evidence against a stored leg for material difference.
///
/// Failed evidence compares identity and owned legs only, because it carries no fill economics.
fn leg_evidence_conflicts(
    stored: &SettlementLeg,
    incoming: &AdmittedLeg,
    incoming_has_economics: bool,
) -> bool {
    stored.venue_order_id != incoming.venue_order_id
        || stored.instrument_id != incoming.instrument_id
        || stored.order_side != incoming.order_side
        || stored.liquidity_side != incoming.liquidity_side
        || stored.trade_id != incoming.trade_id
        || (incoming_has_economics && admitted_economics_diverge(stored, incoming))
}

fn stream_application_eligible(
    inner: &RegistryInner,
    record: &SettlementRecord,
    new_legs: &[AdmittedLeg],
) -> bool {
    inner.live
        && record.admitted_session == inner.session
        && new_legs
            .iter()
            .all(|leg| inner.session_orders.contains(&leg.venue_order_id))
        && record
            .legs
            .iter()
            .all(|leg| inner.session_orders.contains(&leg.venue_order_id))
}

fn apply_absent_legs(
    record: &mut SettlementRecord,
    admitted: &AdmittedTrade,
    actions: &mut Vec<SettlementAction>,
) {
    for incoming in &admitted.legs {
        let Some(leg) = record
            .legs
            .iter_mut()
            .find(|leg| leg.venue_order_id == incoming.venue_order_id && leg.awaits_application())
        else {
            continue;
        };

        // Authorization is consumed here, so a later stream copy cannot emit the leg again
        // while its fill waits in the buffer
        leg.authorized = true;
        actions.push(SettlementAction::ApplyLeg {
            venue_trade_id: record.venue_trade_id.clone(),
            leg: incoming.clone(),
        });
    }
}

fn enter_quarantined(record: &mut SettlementRecord) {
    if record.settlement != SettlementState::Quarantined {
        log::warn!(
            "Quarantining Polymarket trade {} pending targeted terminal REST resolution",
            record.venue_trade_id
        );
        record.settlement = SettlementState::Quarantined;
    }
}

fn admitted_economics_diverge(stored: &SettlementLeg, incoming: &AdmittedLeg) -> bool {
    stored.last_qty != incoming.last_qty
        || stored.last_px != incoming.last_px
        || stored.commission != incoming.commission
        || stored.ts_event != incoming.ts_event
}

fn legs_materially_equal(previous: &[AdmittedLeg], incoming: &[AdmittedLeg]) -> bool {
    // Fee basis is local metadata, not venue evidence. Two schedules can floor to the same
    // commission, and a basis-less restored fill must not hard-fault on that difference alone.
    fn without_basis(leg: &AdmittedLeg) -> AdmittedLeg {
        AdmittedLeg {
            taker_fee_basis: None,
            ..leg.clone()
        }
    }

    previous.len() == incoming.len()
        && previous.iter().all(|leg| {
            let leg = without_basis(leg);
            incoming.iter().any(|other| without_basis(other) == leg)
        })
}

/// Marks an order for a targeted REST read of trades missed during a stream gap, unless it already
/// awaits a read; an unknown submit outcome's read covers trades along with its status.
fn insert_stream_gap_order(
    uncertain_orders: &mut AHashMap<VenueOrderId, UncertainOrder>,
    venue_order_id: VenueOrderId,
    instrument_id: InstrumentId,
    noted_at: UnixNanos,
) {
    uncertain_orders
        .entry(venue_order_id)
        .or_insert(UncertainOrder {
            instrument_id,
            noted_at,
            kind: UncertainOrderKind::StreamGap,
        });
}

fn hard_fault(inner: &mut RegistryInner, key: &str, reason: String) {
    let Some(record) = inner.records.get_mut(key) else {
        return;
    };

    if record.hard_fault.is_some() {
        return;
    }

    log::error!("Polymarket settlement trade {key} entered hard fault: {reason}");
    record.hard_fault = Some(reason);
    inner.account_refresh_requested = true;
}

fn fault_client(inner: &mut RegistryInner, reason: String) {
    if inner.client_fault.is_none() {
        log::error!("Polymarket execution client faulting closed: {reason}");
        inner.client_fault = Some(reason);
    }
}

/// Reserves room for one new record or unbound leg. Hydration entries do not count toward the
/// capacity; exhausting it afterwards faults the client closed.
fn reserve_entry(inner: &mut RegistryInner) -> bool {
    if !inner.live {
        inner.hydrated += 1;
        return true;
    }

    if inner.records.len() + inner.unbound_legs.len() < inner.hydrated + MAX_SETTLEMENT_RECORDS {
        return true;
    }

    fault_client(
        inner,
        format!("settlement registry capacity of {MAX_SETTLEMENT_RECORDS} records exhausted"),
    );
    false
}

/// Ensures a record exists for `key`; returns `false` when capacity exhaustion faulted the
/// client instead.
fn ensure_record(inner: &mut RegistryInner, key: &str, admitted_session: u64) -> bool {
    if inner.records.contains_key(key) {
        return true;
    }

    if !reserve_entry(inner) {
        return false;
    }

    inner.records.insert(
        key.to_string(),
        SettlementRecord::new(key.to_string(), admitted_session),
    );
    true
}

/// Moves unbound observed legs into the record that venue evidence now identifies, matching on
/// the deterministic leg trade ID.
fn bind_unbound_legs(inner: &mut RegistryInner, key: &str, legs: &[AdmittedLeg]) {
    for admitted in legs {
        let Some(leg) = inner.unbound_legs.remove(&admitted.trade_id) else {
            continue;
        };

        inner
            .leg_trade_ids
            .insert(admitted.trade_id, key.to_string());

        if let Some(record) = inner.records.get_mut(key) {
            record.legs.push(leg);
        }
    }
}

fn order_report_fills(order: &OrderAny) -> anyhow::Result<Vec<(&OrderFilled, Decimal, bool)>> {
    let inferred: AHashSet<_> = inferred_reconciliation_trade_ids(order)?
        .into_iter()
        .collect();
    let events = order.events();

    let corrections: AHashMap<_, _> = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::FillVoided(voided) => Some((voided.trade_id, voided.voided_qty)),
            _ => None,
        })
        .collect();

    Ok(events
        .into_iter()
        .filter_map(|event| {
            let OrderEventAny::Filled(fill) = event else {
                return None;
            };

            let removed = corrections
                .get(&fill.trade_id)
                .map_or(Decimal::ZERO, |quantity| quantity.as_decimal())
                .min(fill.last_qty.as_decimal());
            Some((
                fill,
                fill.last_qty.as_decimal() - removed,
                inferred.contains(&fill.trade_id),
            ))
        })
        .collect())
}

fn record_observed_fill(inner: &mut RegistryInner, fill: &OrderFilled) {
    if let Some(leg) = observed_leg_mut(inner, fill) {
        if !matches!(
            leg.application,
            LegApplication::VoidPending | LegApplication::VoidObserved
        ) {
            leg.application = LegApplication::FillObserved;
        }

        leg.authorized = false;
        leg.applied_fill = Some(Box::new(fill.clone()));
    }
}

fn record_observed_void(inner: &mut RegistryInner, voided: &OrderFillVoided) {
    if let Some(leg) = observed_leg_mut(inner, &voided_event_fill(voided)) {
        leg.application = LegApplication::VoidObserved;
        leg.authorized = false;
    }
}

/// Returns the leg for an observed core event, creating it when unknown: in its trade record
/// when the venue trade ID is derivable, otherwise as an unbound leg. Returns `None` only when
/// capacity exhaustion faulted the client.
fn observed_leg_mut<'a>(
    inner: &'a mut RegistryInner,
    fill: &OrderFilled,
) -> Option<&'a mut SettlementLeg> {
    let trade_id = fill.trade_id;
    let key = inner
        .leg_trade_ids
        .get(&trade_id)
        .cloned()
        .or_else(|| observed_trade_key(fill));

    let Some(key) = key else {
        if !inner.unbound_legs.contains_key(&trade_id) {
            if !reserve_entry(inner) {
                return None;
            }

            inner.unbound_legs.insert(trade_id, observed_leg(fill));
        }

        return inner.unbound_legs.get_mut(&trade_id);
    };

    if !ensure_record(inner, &key, 0) {
        return None;
    }

    inner.leg_trade_ids.insert(trade_id, key.clone());
    let record = inner.records.get_mut(&key)?;
    if record.leg_by_trade_id(&trade_id).is_none() {
        record.legs.push(observed_leg(fill));
    }

    record.leg_by_trade_id(&trade_id)
}

/// Derives the venue trade key for an observed core event.
///
/// Trade-sourced fills carry the raw venue trade ID in `info["id"]`, and a taker leg's trade ID
/// is the venue trade ID itself. A maker fill rebuilt from a fill report carries only its
/// composite leg trade ID, which is not invertible, so it has no derivable key.
fn observed_trade_key(fill: &OrderFilled) -> Option<String> {
    if let Some(id) = fill
        .info
        .as_ref()
        .and_then(|info| info.get(&Ustr::from("id")))
    {
        return Some(id.to_string());
    }

    (fill.liquidity_side == LiquiditySide::Taker).then(|| fill.trade_id.to_string())
}

fn observed_leg(fill: &OrderFilled) -> SettlementLeg {
    SettlementLeg {
        venue_order_id: fill.venue_order_id,
        trade_id: fill.trade_id,
        instrument_id: fill.instrument_id,
        order_side: fill.order_side,
        liquidity_side: fill.liquidity_side,
        last_qty: fill.last_qty,
        last_px: fill.last_px,
        commission: fill
            .commission
            .clone()
            .unwrap_or_else(|| Money::zero(fill.currency.clone())),
        taker_fee_basis: None,
        ts_event: fill.ts_event,
        application: LegApplication::FillObserved,
        applied_fill: Some(Box::new(fill.clone())),
        authorized: false,
        report_routed: false,
        venue_evidence: false,
    }
}

fn voided_event_fill(voided: &OrderFillVoided) -> OrderFilled {
    OrderFilled::new(
        voided.trader_id,
        voided.strategy_id,
        voided.instrument_id,
        voided.client_order_id,
        voided.venue_order_id,
        voided.account_id,
        voided.trade_id,
        voided.order_side,
        voided.order_type,
        voided.voided_qty,
        voided.last_px,
        voided.currency.clone(),
        voided.liquidity_side,
        voided.event_id,
        voided.ts_event,
        voided.ts_init,
        false,
        voided.position_id,
        voided.commission_voided.clone(),
        voided.info.clone(),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use futures_util::FutureExt;
    use indexmap::IndexMap;
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_execution::reconciliation::create_inferred_reconciliation_trade_id;
    use nautilus_model::{
        enums::{OrderSide, OrderType},
        identifiers::{PositionId, StrategyId, TraderId},
        orders::{builder::OrderTestBuilder, stubs::TestOrderEventStubs},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::execution::{get_pusd_currency, parse::make_composite_trade_id};

    pub(crate) fn settlement_state(
        registry: &SettlementRegistry,
        venue_trade_id: &str,
    ) -> Option<SettlementState> {
        registry
            .inner
            .lock()
            .records
            .get(venue_trade_id)
            .map(|record| record.settlement)
    }

    pub(crate) fn leg_application(
        registry: &SettlementRegistry,
        trade_id: &TradeId,
    ) -> Option<LegApplication> {
        let inner = registry.inner.lock();
        if let Some(leg) = inner.unbound_legs.get(trade_id) {
            return Some(leg.application);
        }

        let key = inner.leg_trade_ids.get(trade_id)?;
        inner
            .records
            .get(key)?
            .legs
            .iter()
            .find_map(|leg| (leg.trade_id == *trade_id).then_some(leg.application))
    }

    pub(crate) fn trade_hard_fault(
        registry: &SettlementRegistry,
        venue_trade_id: &str,
    ) -> Option<String> {
        registry
            .inner
            .lock()
            .records
            .get(venue_trade_id)
            .and_then(|record| record.hard_fault.clone())
    }

    pub(crate) fn force_client_fault(registry: &SettlementRegistry, reason: &str) {
        fault_client(&mut registry.inner.lock(), reason.to_string());
    }

    const TRADE: &str = "trade-1";

    fn live_registry() -> SettlementRegistry {
        let registry = SettlementRegistry::new(AccountId::from("POLYMARKET-001"));
        registry.begin_session();
        registry
    }

    fn admitted_leg(order: &str, trade_id: &str, liquidity_side: LiquiditySide) -> AdmittedLeg {
        AdmittedLeg {
            venue_order_id: VenueOrderId::from(order),
            trade_id: TradeId::from(trade_id),
            instrument_id: InstrumentId::from("TOKEN-A.POLYMARKET"),
            order_side: OrderSide::Buy,
            liquidity_side,
            last_qty: Quantity::from("10.00"),
            last_px: Price::from("0.50"),
            commission: Money::zero(get_pusd_currency()),
            taker_fee_basis: None,
            ts_event: UnixNanos::from(1_000_u64),
        }
    }

    fn taker_leg() -> AdmittedLeg {
        admitted_leg("0xtaker", TRADE, LiquiditySide::Taker)
    }

    fn maker_leg(order: &str) -> AdmittedLeg {
        admitted_leg(order, &format!("{TRADE}-{order}"), LiquiditySide::Maker)
    }

    fn trade(status: PolymarketTradeStatus, legs: Vec<AdmittedLeg>) -> AdmittedTrade {
        AdmittedTrade {
            venue_trade_id: TRADE.to_string(),
            status,
            legs,
        }
    }

    fn applied_fill(leg: &AdmittedLeg, venue_trade_id: Option<&str>) -> OrderFilled {
        let info = venue_trade_id.map(|id| {
            let mut info = IndexMap::new();
            info.insert(Ustr::from("id"), Ustr::from(id));
            info
        });

        OrderFilled::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            leg.instrument_id,
            ClientOrderId::from("O-001"),
            leg.venue_order_id,
            AccountId::from("POLYMARKET-001"),
            leg.trade_id,
            leg.order_side,
            OrderType::Limit,
            leg.last_qty,
            leg.last_px,
            get_pusd_currency(),
            leg.liquidity_side,
            UUID4::new(),
            leg.ts_event,
            leg.ts_event,
            false,
            None,
            Some(leg.commission.clone()),
            info,
        )
    }

    #[rstest]
    #[case::unbound(false, "trade-1-0xmaker-a")]
    #[case::bound(true, "trade-1-0xmaker-a")]
    #[case::venue_uuid(false, "2d89666b-1a1e-5a75-b193-4eb3b454c757")]
    fn test_report_filled_quantities_union_observed_and_reported_legs(
        #[case] bound: bool,
        #[case] trade_id: &str,
    ) {
        let registry = live_registry();
        let mut observed = maker_leg("0xmaker-a");
        observed.trade_id = TradeId::from(trade_id);
        let mut reported = observed.clone();
        reported.trade_id = TradeId::from("trade-2-0xmaker-a");
        reported.last_qty = Quantity::from("3.00");

        if bound {
            registry.admit_rest_result(&trade(
                PolymarketTradeStatus::Confirmed,
                vec![observed.clone()],
            ));
        }

        let mut fill = applied_fill(&observed, None);
        fill.reconciliation = true;
        registry.observe_fill_applied(&fill);
        let order = filled_order(&fill, &[]);
        let reports = [
            observed.fill_report(registry.account_id, UnixNanos::from(1)),
            reported.fill_report(registry.account_id, UnixNanos::from(1)),
        ];

        let quantities = registry
            .report_filled_quantities(&reports, [&order])
            .unwrap();

        assert_eq!(
            quantities,
            AHashMap::from_iter([(observed.venue_order_id, Decimal::from(13))]),
        );
        assert_eq!(
            registry.report_filled_quantities(&[], [&order]).unwrap(),
            AHashMap::from_iter([(observed.venue_order_id, Decimal::from(10))]),
        );
    }

    #[rstest]
    #[case::single(false)]
    #[case::incremental(true)]
    fn test_report_filled_quantities_do_not_add_inferred_fill_to_its_venue_report(
        #[case] incremental: bool,
    ) {
        let registry = live_registry();
        let leg = maker_leg("0xmaker-a");
        let mut inferred = applied_fill(&leg, None);
        inferred.reconciliation = true;
        let position_id = PositionId::from("TOKEN-A.POLYMARKET-S-001");
        inferred.position_id = Some(position_id);
        inferred.trade_id = create_inferred_reconciliation_trade_id(
            inferred.account_id,
            inferred.instrument_id,
            inferred.client_order_id,
            Some(inferred.venue_order_id),
            inferred.order_side,
            inferred.order_type,
            inferred.last_qty,
            inferred.last_qty,
            inferred.last_px,
            position_id,
            inferred.ts_event,
        );
        registry.observe_fill_applied(&inferred);
        let mut order = filled_order(&inferred, &[]);
        let mut reports = vec![leg.fill_report(registry.account_id, UnixNanos::from(1))];

        let expected = if incremental {
            let mut next = inferred;
            next.last_qty = Quantity::from("3.00");
            next.ts_event = UnixNanos::from(2_000);
            next.trade_id = create_inferred_reconciliation_trade_id(
                next.account_id,
                next.instrument_id,
                next.client_order_id,
                Some(next.venue_order_id),
                next.order_side,
                next.order_type,
                Quantity::from("13.00"),
                next.last_qty,
                next.last_px,
                position_id,
                next.ts_event,
            );
            order.apply(OrderEventAny::Filled(next.clone())).unwrap();
            registry.observe_fill_applied(&next);
            let mut report = leg.fill_report(registry.account_id, UnixNanos::from(1));
            report.trade_id = TradeId::from("trade-2-0xmaker-a");
            report.last_qty = next.last_qty;
            reports.push(report);
            Decimal::from(13)
        } else {
            Decimal::from(10)
        };

        let quantities = registry
            .report_filled_quantities(&reports, [&order])
            .unwrap();

        assert_eq!(
            quantities,
            AHashMap::from_iter([(leg.venue_order_id, expected)]),
        );
    }

    #[rstest]
    #[case::runtime(false)]
    #[case::hydrated(true)]
    fn test_report_filled_quantities_retain_partial_void_remainder(
        #[case] hydrated: bool,
        #[values(false, true)] reported: bool,
        #[values(3, 5, 10)] cumulative_voided: u32,
    ) {
        let registry = live_registry();
        let leg = maker_leg("0xmaker-a");
        let fill = applied_fill(&leg, None);
        let mut voided = applied_void(&fill);
        voided.voided_qty = Quantity::from("3.00");

        if hydrated {
            registry.hydrate_fill(&fill);
            registry.hydrate_void(&voided);
        } else {
            registry.observe_fill_applied(&fill);
            registry.observe_void_applied(&voided);
        }

        let mut voids = vec![voided];

        if cumulative_voided > 3 {
            let mut next = applied_void(&fill);
            next.correction_id = Ustr::from("void-2");
            next.voided_qty = Quantity::from(format!("{cumulative_voided}.00").as_str());

            if hydrated {
                registry.hydrate_void(&next);
            } else {
                registry.observe_void_applied(&next);
            }

            voids.push(next);
        }

        let order = filled_order(&fill, &voids);

        let reports = if reported {
            vec![leg.fill_report(registry.account_id, UnixNanos::from(1))]
        } else {
            Vec::new()
        };

        let quantities = registry
            .report_filled_quantities(&reports, [&order])
            .unwrap();

        assert_eq!(
            quantities,
            AHashMap::from_iter([(leg.venue_order_id, Decimal::from(10 - cumulative_voided))]),
        );
        assert_eq!(
            order.filled_qty().as_decimal(),
            Decimal::from(10 - cumulative_voided)
        );
    }

    #[rstest]
    #[case::runtime(false)]
    #[case::hydrated(true)]
    fn test_report_filled_quantities_reject_failed_partial_void_remainder(
        #[case] hydrated: bool,
        #[values(false, true)] fully_corrected: bool,
        #[values(false, true)] reported: bool,
    ) {
        let registry = live_registry();
        let leg = maker_leg("0xmaker-a");
        let fill = applied_fill(&leg, None);
        let mut partial = applied_void(&fill);
        partial.voided_qty = Quantity::from("3.00");

        if hydrated {
            registry.hydrate_fill(&fill);
            registry.hydrate_void(&partial);
        } else {
            registry.observe_fill_applied(&fill);
            registry.observe_void_applied(&partial);
        }

        let actions =
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));
        let mut voids = vec![partial];

        if fully_corrected {
            let mut complete = applied_void(&fill);
            complete.correction_id = Ustr::from("void-2");
            registry.observe_void_applied(&complete);
            voids.push(complete);
        }

        let order = filled_order(&fill, &voids);
        let mut distinct = leg.fill_report(registry.account_id, UnixNanos::from(1));
        distinct.trade_id = TradeId::from("trade-2-0xmaker-a");
        distinct.last_qty = Quantity::from("3.00");

        let reports = if reported { vec![distinct] } else { Vec::new() };
        let quantities = registry.report_filled_quantities(&reports, [&order]);

        assert!(actions.is_empty());
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::RestFailed)
        );
        assert_eq!(
            leg_application(&registry, &leg.trade_id),
            Some(LegApplication::VoidObserved)
        );

        if fully_corrected {
            let expected = if reported {
                Decimal::from(3)
            } else {
                Decimal::ZERO
            };

            assert_eq!(
                quantities.unwrap(),
                AHashMap::from_iter([(leg.venue_order_id, expected)])
            );
        } else {
            assert!(
                quantities
                    .unwrap_err()
                    .to_string()
                    .contains("retains quantity after targeted REST FAILED")
            );
        }

        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::RestFailed)
        );
    }

    #[rstest]
    #[case::changed_leg(maker_leg("0xmaker-a"), "contradicts retained leg")]
    #[case::added_leg(maker_leg("0xmaker-c"), "adds a leg to retained terminal settlement")]
    fn test_scoped_report_validation_preserves_complete_terminal_outcome(
        #[case] mut changed: AdmittedLeg,
        #[case] expected_error: &str,
    ) {
        let registry = live_registry();
        let first = maker_leg("0xmaker-a");
        let second = maker_leg("0xmaker-b");
        registry.admit_rest_result(&trade(
            PolymarketTradeStatus::Confirmed,
            vec![first.clone(), second.clone()],
        ));

        registry
            .build_fill_report(TRADE, &first, UnixNanos::from(1))
            .unwrap();
        registry
            .build_fill_report(TRADE, &second, UnixNanos::from(1))
            .unwrap();
        changed.last_qty = Quantity::from("9.00");
        let error = registry
            .build_fill_report(TRADE, &changed, UnixNanos::from(1))
            .unwrap_err();

        assert!(error.to_string().contains(expected_error));
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::RestConfirmed)
        );
        assert_eq!(trade_hard_fault(&registry, TRADE), None);
        assert_eq!(registry.record_count(), 1);
        assert_eq!(
            registry.inner.lock().records[TRADE].terminal_rest_legs,
            Some(vec![first, second])
        );
    }

    #[rstest]
    fn test_unknown_report_validation_does_not_establish_terminal_settlement() {
        let registry = live_registry();

        registry
            .build_fill_report(TRADE, &taker_leg(), UnixNanos::from(1))
            .unwrap();

        assert_eq!(registry.record_count(), 0);
        assert_eq!(settlement_state(&registry, TRADE), None);
    }

    #[rstest]
    #[case::bound(true)]
    #[case::unbound(false)]
    fn test_report_preserves_observed_core_economics(#[case] bound: bool) {
        let registry = live_registry();
        let leg = maker_leg("0xmaker-a");
        registry.hydrate_fill(&applied_fill(&leg, bound.then_some(TRADE)));
        let mut incoming = leg.clone();
        incoming.last_qty = Quantity::from("11.00");
        incoming.last_px = Price::from("0.60");
        incoming.commission = Money::from("0.01 pUSD");
        incoming.ts_event = UnixNanos::from(2_000);

        let report = registry
            .build_fill_report(TRADE, &incoming, UnixNanos::from(3_000))
            .unwrap();

        assert_eq!(report.account_id, registry.account_id);
        assert_eq!(report.instrument_id, leg.instrument_id);
        assert_eq!(report.venue_order_id, leg.venue_order_id);
        assert_eq!(report.trade_id, leg.trade_id);
        assert_eq!(report.order_side, leg.order_side);
        assert_eq!(report.liquidity_side, leg.liquidity_side);
        assert_eq!(report.last_qty, leg.last_qty);
        assert_eq!(report.last_px, leg.last_px);
        assert_eq!(report.commission, leg.commission);
        assert_eq!(report.ts_event, leg.ts_event);
        assert_eq!(report.ts_init, UnixNanos::from(3_000));
        assert_eq!(report.client_order_id, None);
        assert_eq!(report.venue_position_id, None);
        assert_eq!(report.avg_px, None);
    }

    fn applied_void(fill: &OrderFilled) -> OrderFillVoided {
        OrderFillVoided::new(
            fill.trader_id,
            fill.strategy_id,
            fill.instrument_id,
            fill.client_order_id,
            fill.venue_order_id,
            fill.account_id,
            Ustr::from("void-1"),
            fill.trade_id,
            fill.last_qty,
            fill.commission.clone(),
            fill.order_side,
            fill.order_type,
            fill.last_px,
            fill.currency.clone(),
            fill.liquidity_side,
            None,
            None,
            fill.info.clone(),
            UUID4::new(),
            fill.ts_event,
            fill.ts_event,
            false,
            false,
        )
    }

    fn filled_order(fill: &OrderFilled, voids: &[OrderFillVoided]) -> OrderAny {
        let mut order = OrderTestBuilder::new(fill.order_type)
            .trader_id(fill.trader_id)
            .strategy_id(fill.strategy_id)
            .client_order_id(fill.client_order_id)
            .instrument_id(fill.instrument_id)
            .side(fill.order_side)
            .quantity(Quantity::from("100.00"))
            .price(fill.last_px)
            .build();
        order
            .apply(TestOrderEventStubs::submitted(&order, fill.account_id))
            .unwrap();
        order.apply(OrderEventAny::Filled(fill.clone())).unwrap();

        for voided in voids {
            order
                .apply(OrderEventAny::FillVoided(voided.clone()))
                .unwrap();
        }

        order
    }

    fn voided_fill(actions: &[SettlementAction]) -> &OrderFilled {
        actions
            .iter()
            .find_map(|action| match action {
                SettlementAction::VoidAppliedFill { fill, .. } => Some(fill.as_ref()),
                SettlementAction::ApplyLeg { .. } => None,
            })
            .expect("expected a void action")
    }

    fn apply_count(actions: &[SettlementAction]) -> usize {
        actions
            .iter()
            .filter(|action| matches!(action, SettlementAction::ApplyLeg { .. }))
            .count()
    }

    fn voided_trade_ids(actions: &[SettlementAction]) -> Vec<TradeId> {
        actions
            .iter()
            .filter_map(|action| match action {
                SettlementAction::VoidAppliedFill { fill, .. } => Some(fill.trade_id),
                _ => None,
            })
            .collect()
    }

    fn resolution_woken(registry: &SettlementRegistry) -> bool {
        registry.resolution_requested().now_or_never().is_some()
    }

    /// Applies the taker leg provisionally and observes core applying it.
    fn applied_taker(registry: &SettlementRegistry) -> AdmittedLeg {
        let leg = taker_leg();
        registry.note_order_submitted(leg.venue_order_id);
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![leg.clone()]));
        registry.note_leg_enqueued(&leg.trade_id);
        registry.observe_fill_applied(&applied_fill(&leg, Some(TRADE)));
        leg
    }

    #[rstest]
    #[case::matching(|_: &mut AdmittedLeg| {}, false)]
    #[case::quantity(|leg: &mut AdmittedLeg| leg.last_qty = Quantity::from("9.00"), true)]
    #[case::price(|leg: &mut AdmittedLeg| leg.last_px = Price::from("0.60"), true)]
    #[case::commission(|leg: &mut AdmittedLeg| leg.commission = Money::from("0.01 pUSD"), true)]
    #[case::timestamp(|leg: &mut AdmittedLeg| leg.ts_event = UnixNanos::from(2_000), true)]
    #[case::side(|leg: &mut AdmittedLeg| leg.order_side = OrderSide::Sell, true)]
    #[case::instrument(|leg: &mut AdmittedLeg| leg.instrument_id = InstrumentId::from("TOKEN-B.POLYMARKET"), true)]
    #[case::liquidity(|leg: &mut AdmittedLeg| leg.liquidity_side = LiquiditySide::Maker, true)]
    #[case::trade_id(|leg: &mut AdmittedLeg| leg.trade_id = TradeId::from("different-trade"), true)]
    fn test_stream_conflicts_after_fill_application(
        #[case] change: fn(&mut AdmittedLeg),
        #[case] conflicts: bool,
        #[values(PolymarketTradeStatus::Matched, PolymarketTradeStatus::Confirmed)]
        status: PolymarketTradeStatus,
        #[values(false, true)] rest_settled: bool,
    ) {
        let registry = live_registry();
        let original = applied_taker(&registry);

        if rest_settled {
            registry.admit_rest_result(&trade(
                PolymarketTradeStatus::Confirmed,
                vec![original.clone()],
            ));
        }

        let mut incoming = original.clone();
        change(&mut incoming);

        let actions = registry.admit_stream_trade(&trade(status, vec![incoming]));

        let expected = if rest_settled {
            SettlementState::RestConfirmed
        } else if conflicts {
            SettlementState::Quarantined
        } else if status == PolymarketTradeStatus::Confirmed {
            SettlementState::StreamConfirmed
        } else {
            SettlementState::Provisional
        };

        assert!(actions.is_empty());
        assert_eq!(settlement_state(&registry, TRADE), Some(expected));
        assert_eq!(trade_hard_fault(&registry, TRADE), None);
        assert_eq!(
            registry.pending_resolutions(),
            if conflicts {
                vec![TRADE.to_string()]
            } else {
                vec![]
            }
        );
        assert_eq!(
            registry.ensure_resolved(None, "mass status").is_ok(),
            !conflicts
        );
        assert_eq!(
            leg_application(&registry, &original.trade_id),
            Some(LegApplication::FillObserved)
        );
        let inner = registry.inner.lock();
        let stored = &inner.records[TRADE].legs[0];
        assert_eq!(stored.last_qty, original.last_qty);
        assert_eq!(stored.last_px, original.last_px);
        assert_eq!(stored.commission, original.commission);
        assert_eq!(stored.ts_event, original.ts_event);
        assert_eq!(stored.trade_id, original.trade_id);
        assert_eq!(stored.venue_order_id, original.venue_order_id);
        assert_eq!(stored.instrument_id, original.instrument_id);
        assert_eq!(stored.order_side, original.order_side);
        assert_eq!(stored.liquidity_side, original.liquidity_side);
    }

    #[rstest]
    #[case::refreshed_commission(false, false)]
    #[case::changed_quantity(true, false)]
    #[case::changed_timestamp(false, true)]
    fn test_retained_basis_does_not_hide_a_different_admitted_commission(
        #[case] change_quantity: bool,
        #[case] change_timestamp: bool,
    ) {
        let registry = live_registry();
        let mut original = taker_leg();
        original.taker_fee_basis = Some(TakerFeeBasis {
            rate: rust_decimal::Decimal::ONE,
            exponent: rust_decimal::Decimal::ONE,
        });

        original.commission = Money::from("0.45062 pUSD");
        registry.note_order_submitted(original.venue_order_id);
        registry.admit_stream_trade(&trade(
            PolymarketTradeStatus::Matched,
            vec![original.clone()],
        ));
        registry.note_leg_enqueued(&original.trade_id);
        registry.observe_fill_applied(&applied_fill(&original, Some(TRADE)));

        let mut incoming = original.clone();
        incoming.commission = Money::from("0.06250 pUSD");
        incoming.taker_fee_basis = Some(TakerFeeBasis {
            rate: rust_decimal::Decimal::new(1, 2),
            exponent: rust_decimal::Decimal::ONE,
        });

        if change_quantity {
            incoming.last_qty = Quantity::from("9.00");
        }

        if change_timestamp {
            incoming.ts_event = UnixNanos::from(2_000);
        }

        // A different admitted commission is venue-comparable evidence. The retained basis
        // does not replace it.
        let conflicts = true;

        let stream = registry.admit_stream_trade(&trade(
            PolymarketTradeStatus::Confirmed,
            vec![incoming.clone()],
        ));
        let first_rest = registry.admit_rest_result(&trade(
            PolymarketTradeStatus::Confirmed,
            vec![incoming.clone()],
        ));
        let mut repeated = incoming;
        repeated.commission = Money::from("0.10000 pUSD");
        let second_rest =
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Confirmed, vec![repeated]));

        assert!(stream.is_empty());
        assert!(first_rest.is_empty());
        assert!(second_rest.is_empty());
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::RestConfirmed)
        );
        assert_eq!(trade_hard_fault(&registry, TRADE).is_some(), conflicts);
        assert_eq!(
            registry.ensure_resolved(None, "mass status").is_ok(),
            !conflicts
        );
        assert_eq!(
            registry.inner.lock().records[TRADE].legs[0].commission,
            original.commission
        );
        assert_eq!(
            leg_application(&registry, &original.trade_id),
            Some(LegApplication::FillObserved)
        );
    }

    #[rstest]
    fn test_repeated_rest_ignores_fee_basis_when_commission_matches() {
        let registry = live_registry();
        registry.mark_hydrating();
        let historical = taker_leg();
        registry.hydrate_fill(&applied_fill(&historical, Some(TRADE)));
        registry.mark_live();

        let mut first = historical.clone();
        first.taker_fee_basis = Some(TakerFeeBasis {
            rate: rust_decimal::Decimal::ZERO,
            exponent: rust_decimal::Decimal::ONE,
        });

        assert!(
            registry
                .admit_rest_result(&trade(PolymarketTradeStatus::Confirmed, vec![first]))
                .is_empty()
        );

        let mut second = historical;
        second.taker_fee_basis = Some(TakerFeeBasis {
            rate: rust_decimal::Decimal::ZERO,
            exponent: rust_decimal::Decimal::TWO,
        });

        assert!(
            registry
                .admit_rest_result(&trade(PolymarketTradeStatus::Confirmed, vec![second]))
                .is_empty()
        );

        assert_eq!(trade_hard_fault(&registry, TRADE), None);
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::RestConfirmed)
        );
        assert!(registry.ensure_resolved(None, "mass status").is_ok());
    }

    #[rstest]
    fn test_retained_basis_keeps_rest_quantity_change_on_unattempted_leg() {
        let registry = live_registry();
        let mut original = taker_leg();
        original.taker_fee_basis = Some(TakerFeeBasis {
            rate: rust_decimal::Decimal::ONE,
            exponent: rust_decimal::Decimal::ONE,
        });

        original.commission = Money::from("0.45062 pUSD");
        registry.admit_stream_trade(&trade(
            PolymarketTradeStatus::Matched,
            vec![original.clone()],
        ));

        let mut incoming = original;
        incoming.last_qty = Quantity::from("9.00");
        incoming.commission = Money::from("0.20000 pUSD");
        let actions = registry.admit_rest_result(&trade(
            PolymarketTradeStatus::Confirmed,
            vec![incoming.clone()],
        ));

        assert_eq!(apply_count(&actions), 1);
        assert_eq!(trade_hard_fault(&registry, TRADE), None);
        let inner = registry.inner.lock();
        let stored = &inner.records[TRADE].legs[0];
        assert_eq!(stored.last_qty, incoming.last_qty);
        assert_eq!(stored.commission, incoming.commission);
    }

    #[rstest]
    #[case::matching(|_: &mut AdmittedLeg| {}, false)]
    #[case::quantity(|leg: &mut AdmittedLeg| leg.last_qty = Quantity::from("9.00"), true)]
    #[case::price(|leg: &mut AdmittedLeg| leg.last_px = Price::from("0.60"), true)]
    #[case::commission(|leg: &mut AdmittedLeg| leg.commission = Money::from("0.01 pUSD"), true)]
    #[case::timestamp(|leg: &mut AdmittedLeg| leg.ts_event = UnixNanos::from(2_000), true)]
    #[case::side(|leg: &mut AdmittedLeg| leg.order_side = OrderSide::Sell, true)]
    #[case::instrument(|leg: &mut AdmittedLeg| leg.instrument_id = InstrumentId::from("TOKEN-B.POLYMARKET"), true)]
    #[case::liquidity(|leg: &mut AdmittedLeg| leg.liquidity_side = LiquiditySide::Maker, true)]
    #[case::trade_id(|leg: &mut AdmittedLeg| leg.trade_id = TradeId::from("different-trade"), true)]
    fn test_first_rest_confirmation_checks_full_leg_evidence(
        #[case] change: fn(&mut AdmittedLeg),
        #[case] conflicts: bool,
        #[values(
            LegApplication::Absent,
            LegApplication::FillPending,
            LegApplication::FillObserved
        )]
        application: LegApplication,
    ) {
        let registry = live_registry();
        let original = taker_leg();
        registry.note_order_submitted(original.venue_order_id);
        registry.admit_stream_trade(&trade(
            PolymarketTradeStatus::Matched,
            vec![original.clone()],
        ));

        if application != LegApplication::Absent {
            registry.note_leg_enqueued(&original.trade_id);
        }

        if application == LegApplication::FillObserved {
            registry.observe_fill_applied(&applied_fill(&original, Some(TRADE)));
        }

        let mut incoming = original.clone();
        change(&mut incoming);

        let actions = registry.admit_rest_result(&trade(
            PolymarketTradeStatus::Confirmed,
            vec![incoming.clone()],
        ));
        let duplicate = registry.admit_rest_result(&trade(
            PolymarketTradeStatus::Confirmed,
            vec![incoming.clone()],
        ));

        assert!(actions.is_empty());
        assert!(duplicate.is_empty());
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::RestConfirmed)
        );
        let expected_fault = conflicts.then(|| format!("terminal REST CONFIRMED evidence for trade {TRADE} contradicts the applied fill on order {}", original.venue_order_id));
        assert_eq!(trade_hard_fault(&registry, TRADE), expected_fault);
        assert_eq!(
            registry.ensure_resolved(None, "mass status").is_ok(),
            !conflicts && application != LegApplication::FillPending
        );
        assert_eq!(
            registry.inner.lock().records[TRADE].terminal_rest_legs,
            Some(vec![incoming])
        );
        assert_eq!(
            leg_application(&registry, &original.trade_id),
            Some(application)
        );
    }

    #[rstest]
    #[case::new_leg(false)]
    #[case::unattempted_leg(true)]
    fn test_rest_identity_conflict_suppresses_other_legs(
        #[case] stored: bool,
        #[values(false, true)] conflict_first: bool,
    ) {
        let registry = live_registry();
        let original = applied_taker(&registry);
        let original_fill = registry.inner.lock().records[TRADE].legs[0]
            .applied_fill
            .clone();
        let other = maker_leg("0xmaker-a");

        if stored {
            registry.admit_stream_trade(&trade(
                PolymarketTradeStatus::Matched,
                vec![original.clone(), other.clone()],
            ));
        }

        let mut incoming = original.clone();
        incoming.order_side = OrderSide::Sell;

        let legs = if conflict_first {
            vec![incoming, other]
        } else {
            vec![other, incoming]
        };

        let actions =
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Confirmed, legs.clone()));
        let duplicate =
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Confirmed, legs.clone()));

        assert!(actions.is_empty());
        assert!(duplicate.is_empty());
        assert_eq!(
            trade_hard_fault(&registry, TRADE),
            Some(format!(
                "terminal REST CONFIRMED evidence for trade {TRADE} contradicts the applied \
                 fill on order {}",
                original.venue_order_id,
            ))
        );
        assert!(registry.ensure_resolved(None, "mass status").is_err());
        let inner = registry.inner.lock();
        let record = &inner.records[TRADE];
        assert_eq!(record.terminal_rest_legs, Some(legs));
        assert_eq!(record.legs[0].application, LegApplication::FillObserved);
        assert_eq!(record.legs[0].applied_fill, original_fill);
    }

    #[rstest]
    #[case::observed(false, false)]
    #[case::declined(true, false)]
    #[case::report_routed(false, true)]
    fn test_rest_confirmed_replaces_unattempted_leg_evidence(
        #[case] declined: bool,
        #[case] report_routed: bool,
    ) {
        let registry = live_registry();
        let mut original = taker_leg();
        original.commission = Money::from("0.02 pUSD");
        registry.admit_stream_trade(&trade(
            PolymarketTradeStatus::Matched,
            vec![original.clone()],
        ));

        let incoming = AdmittedLeg {
            venue_order_id: original.venue_order_id,
            trade_id: make_composite_trade_id(TRADE, original.venue_order_id.as_str()),
            instrument_id: InstrumentId::from("TOKEN-B.POLYMARKET"),
            order_side: OrderSide::Sell,
            liquidity_side: LiquiditySide::Maker,
            last_qty: Quantity::from("9.00"),
            last_px: Price::from("0.60"),
            commission: Money::zero(get_pusd_currency()),
            taker_fee_basis: None,
            ts_event: UnixNanos::from(2_000),
        };

        let actions = registry.admit_rest_result(&trade(
            PolymarketTradeStatus::Confirmed,
            vec![incoming.clone()],
        ));
        let duplicate = registry.admit_rest_result(&trade(
            PolymarketTradeStatus::Confirmed,
            vec![incoming.clone()],
        ));

        let [
            SettlementAction::ApplyLeg {
                venue_trade_id,
                leg,
            },
        ] = actions.as_slice()
        else {
            panic!("expected one REST fill action");
        };

        assert_eq!(venue_trade_id, TRADE);
        assert_eq!(leg, &incoming);
        assert!(duplicate.is_empty());
        assert_eq!(trade_hard_fault(&registry, TRADE), None);
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::RestConfirmed)
        );
        assert_eq!(registry.pending_resolutions(), Vec::<String>::new());
        assert_eq!(
            registry.inner.lock().records[TRADE].terminal_rest_legs,
            Some(vec![incoming.clone()])
        );
        assert_eq!(leg_application(&registry, &original.trade_id), None);
        assert_eq!(
            leg_application(&registry, &incoming.trade_id),
            Some(LegApplication::Absent)
        );
        {
            let inner = registry.inner.lock();
            assert_eq!(inner.leg_trade_ids.get(&original.trade_id), None);
            assert_eq!(
                inner
                    .leg_trade_ids
                    .get(&incoming.trade_id)
                    .map(String::as_str),
                Some(TRADE)
            );
        }

        if report_routed {
            registry.note_leg_reported(&incoming.trade_id);
            let inner = registry.inner.lock();
            let stored = &inner.records[TRADE].legs[0];
            assert!(stored.report_routed);
            assert!(!stored.authorized);
        } else {
            registry.note_leg_enqueued(&incoming.trade_id);
            assert_eq!(
                leg_application(&registry, &incoming.trade_id),
                Some(LegApplication::FillPending)
            );
            assert!(registry.ensure_resolved(None, "mass status").is_err());
        }

        let fill = applied_fill(&incoming, Some(TRADE));

        if declined {
            registry.observe_fill_declined(&OrderEventAny::Filled(fill));
        } else {
            assert_eq!(registry.observe_fill_applied(&fill), None);
        }

        assert_eq!(
            leg_application(&registry, &incoming.trade_id),
            Some(if declined {
                LegApplication::Absent
            } else {
                LegApplication::FillObserved
            })
        );
        assert_eq!(
            trade_hard_fault(&registry, TRADE),
            declined.then(|| format!(
                "core declined the fill for trade {TRADE} leg {}",
                incoming.trade_id
            ))
        );
        assert_eq!(
            registry.ensure_resolved(None, "mass status").is_ok(),
            !declined
        );
    }

    #[rstest]
    fn test_stream_missing_applied_maker_leg_quarantines_whole_trade() {
        let registry = live_registry();
        let first = maker_leg("0xmaker-a");
        let second = maker_leg("0xmaker-b");
        for leg in [&first, &second] {
            registry.note_order_submitted(leg.venue_order_id);
        }

        registry.admit_stream_trade(&trade(
            PolymarketTradeStatus::Matched,
            vec![first.clone(), second.clone()],
        ));
        registry.note_leg_enqueued(&first.trade_id);
        registry.observe_fill_applied(&applied_fill(&first, Some(TRADE)));

        let actions = registry.admit_stream_trade(&trade(
            PolymarketTradeStatus::Confirmed,
            vec![second.clone()],
        ));

        assert!(actions.is_empty());
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::Quarantined)
        );
        assert_eq!(registry.pending_resolutions(), vec![TRADE.to_string()]);
        assert_eq!(
            leg_application(&registry, &first.trade_id),
            Some(LegApplication::FillObserved)
        );
        assert_eq!(
            leg_application(&registry, &second.trade_id),
            Some(LegApplication::Absent)
        );
        assert_eq!(trade_hard_fault(&registry, TRADE), None);
    }

    #[rstest]
    #[case::core_only(false)]
    #[case::report_routed(true)]
    fn test_restored_normalized_fill_keeps_stream_economics_exemption(#[case] report_routed: bool) {
        let registry = live_registry();
        let mut applied = taker_leg();
        applied.last_qty = Quantity::from("714.285710");

        if report_routed {
            registry.note_order_submitted(applied.venue_order_id);
            registry.admit_stream_trade(&trade(
                PolymarketTradeStatus::Matched,
                vec![applied.clone()],
            ));
            registry.note_leg_reported(&applied.trade_id);
        }

        registry.hydrate_fill(&applied_fill(&applied, Some(TRADE)));
        let mut incoming = applied.clone();
        incoming.last_qty = Quantity::from("714.285714");

        let actions =
            registry.admit_stream_trade(&trade(PolymarketTradeStatus::Confirmed, vec![incoming]));

        assert!(actions.is_empty());
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::StreamConfirmed)
        );
        assert_eq!(registry.pending_resolutions(), Vec::<String>::new());
        assert_eq!(trade_hard_fault(&registry, TRADE), None);
        assert!(registry.ensure_resolved(None, "mass status").is_ok());
        assert_eq!(
            registry.inner.lock().records[TRADE].legs[0].last_qty,
            applied.last_qty
        );
    }

    #[rstest]
    #[case::exact_quantity("714.285714", "7.142850", false)]
    #[case::trimmed_quantity("714.285710", "7.142850", true)]
    #[case::rounded_commission("714.285714", "7.142860", true)]
    fn test_hydrated_legacy_economics_require_terminal_rest_agreement(
        #[case] applied_quantity: &str,
        #[case] applied_commission: &str,
        #[case] conflicts: bool,
    ) {
        let registry = live_registry();
        registry.mark_hydrating();
        let mut historical = taker_leg();
        historical.last_qty = Quantity::from(applied_quantity);
        historical.commission = Money::from_decimal(
            Decimal::from_str_exact(applied_commission).unwrap(),
            get_pusd_currency(),
        )
        .unwrap();
        let fill = applied_fill(&historical, Some(TRADE));
        registry.hydrate_fill(&fill);
        registry.mark_live();
        let mut incoming = historical.clone();
        incoming.last_qty = Quantity::from("714.285714");
        incoming.commission = Money::from("7.142850 pUSD");

        let actions =
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Confirmed, vec![incoming]));

        let expected_fault = conflicts.then(|| format!("terminal REST CONFIRMED evidence for trade {TRADE} contradicts the applied fill on order {}", historical.venue_order_id));
        assert!(actions.is_empty());
        assert_eq!(trade_hard_fault(&registry, TRADE), expected_fault);
        assert_eq!(
            registry.ensure_resolved(None, "mass status").is_ok(),
            !conflicts
        );
        let inner = registry.inner.lock();
        let retained = &inner.records[TRADE].legs[0];
        assert_eq!(retained.last_qty, historical.last_qty);
        assert_eq!(retained.commission, historical.commission);
        assert_eq!(retained.applied_fill.as_deref(), Some(&fill));
        assert_eq!(retained.application, LegApplication::FillObserved);
    }

    #[rstest]
    fn test_core_normalization_preserves_original_venue_evidence() {
        let registry = live_registry();
        let mut venue = taker_leg();
        venue.last_qty = Quantity::from("714.285714");
        registry.note_order_submitted(venue.venue_order_id);
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![venue.clone()]));
        registry.note_leg_enqueued(&venue.trade_id);
        let mut normalized = venue.clone();
        normalized.last_qty = Quantity::from("714.285710");
        let fill = applied_fill(&normalized, Some(TRADE));
        registry.observe_fill_applied(&fill);

        let stream = registry.admit_stream_trade(&trade(
            PolymarketTradeStatus::Confirmed,
            vec![venue.clone()],
        ));
        let rest = registry.admit_rest_result(&trade(
            PolymarketTradeStatus::Confirmed,
            vec![venue.clone()],
        ));

        assert!(stream.is_empty());
        assert!(rest.is_empty());
        assert_eq!(trade_hard_fault(&registry, TRADE), None);
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::RestConfirmed)
        );
        assert!(registry.ensure_resolved(None, "mass status").is_ok());
        let inner = registry.inner.lock();
        let stored = &inner.records[TRADE].legs[0];
        assert_eq!(stored.last_qty, venue.last_qty);
        assert_eq!(stored.applied_fill.as_deref(), Some(&fill));
        assert_eq!(stored.application, LegApplication::FillObserved);
    }

    #[rstest]
    fn test_stream_trade_applies_owned_legs_once_in_current_session() {
        let registry = live_registry();
        let leg = taker_leg();
        registry.note_order_submitted(leg.venue_order_id);

        let first =
            registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![leg.clone()]));
        let replay =
            registry.admit_stream_trade(&trade(PolymarketTradeStatus::Mined, vec![leg.clone()]));
        registry.note_leg_enqueued(&leg.trade_id);
        let pending = leg_application(&registry, &leg.trade_id);
        let void = registry.observe_fill_applied(&applied_fill(&leg, Some(TRADE)));

        assert_eq!(apply_count(&first), 1);
        assert!(replay.is_empty());
        assert_eq!(pending, Some(LegApplication::FillPending));
        assert!(void.is_none());
        assert_eq!(
            leg_application(&registry, &leg.trade_id),
            Some(LegApplication::FillObserved)
        );
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::Provisional)
        );
        assert!(!resolution_woken(&registry));
        assert!(registry.ensure_resolved(None, "mass status").is_ok());
    }

    #[rstest]
    #[case::order_not_noted(false, false)]
    #[case::order_noted_before_reconnect(true, true)]
    fn test_stream_trade_outside_current_session_quarantines(
        #[case] note_order: bool,
        #[case] reconnect: bool,
    ) {
        let registry = live_registry();
        let leg = taker_leg();

        if note_order {
            registry.note_order_submitted(leg.venue_order_id);
        }

        if reconnect {
            registry.begin_session();
        }

        let actions =
            registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![leg]));

        assert_eq!(apply_count(&actions), 0);
        assert!(resolution_woken(&registry));
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::Quarantined)
        );
        assert_eq!(registry.pending_resolutions(), vec![TRADE.to_string()]);
        assert!(registry.ensure_resolved(None, "mass status").is_err());
    }

    #[rstest]
    #[case::same_session(false, 1, vec![])]
    #[case::after_reconnect(true, 0, vec![(
        VenueOrderId::from("0xtaker"),
        UnixNanos::from(9_u64),
        UncertainOrderKind::StreamGap,
    )])]
    fn test_venue_assigned_order_id_inherits_session_note(
        #[case] reconnect: bool,
        #[case] expected_applies: usize,
        #[case] expected_reads: Vec<(VenueOrderId, UnixNanos, UncertainOrderKind)>,
    ) {
        let registry = live_registry();
        let leg = taker_leg();
        let expected = VenueOrderId::from("0xexpected");
        registry.note_order_submitted(expected);

        if reconnect {
            registry.begin_session();
        }

        registry.note_order_accepted(
            expected,
            leg.venue_order_id,
            leg.instrument_id,
            UnixNanos::from(9_u64),
        );
        let reads: Vec<_> = registry
            .uncertain_orders()
            .into_iter()
            .map(|(venue_order_id, order)| (venue_order_id, order.noted_at, order.kind))
            .collect();
        let actions =
            registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![leg]));

        assert_eq!(apply_count(&actions), expected_applies);
        assert_eq!(reads, expected_reads);
    }

    #[rstest]
    fn test_stream_confirmation_never_regresses() {
        let registry = live_registry();
        let leg = applied_taker(&registry);

        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Confirmed, vec![leg.clone()]));
        let stale = registry.admit_stream_trade(&trade(PolymarketTradeStatus::Mined, vec![leg]));

        assert!(stale.is_empty());
        assert!(registry.is_trade_confirmed(TRADE));
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::StreamConfirmed)
        );
    }

    #[rstest]
    fn test_stream_failed_quarantines_without_void() {
        let registry = live_registry();
        let leg = applied_taker(&registry);

        let actions =
            registry.admit_stream_trade(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));

        assert!(voided_trade_ids(&actions).is_empty());
        assert!(resolution_woken(&registry));
        assert_eq!(registry.pending_resolutions(), vec![TRADE.to_string()]);
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::Quarantined)
        );
        assert_eq!(
            leg_application(&registry, &leg.trade_id),
            Some(LegApplication::FillObserved)
        );
    }

    #[rstest]
    fn test_first_rest_failed_voids_observed_legs_and_tombstones_absent_legs() {
        let registry = live_registry();
        let observed = maker_leg("0xmaker-a");
        let absent = maker_leg("0xmaker-b");
        registry.observe_fill_applied(&applied_fill(&observed, Some(TRADE)));
        registry.admit_stream_trade(&trade(
            PolymarketTradeStatus::Failed,
            vec![observed.clone(), absent.clone()],
        ));

        let failed = registry.admit_rest_result(&trade(
            PolymarketTradeStatus::Failed,
            vec![observed.clone(), absent.clone()],
        ));
        let void_pending = leg_application(&registry, &observed.trade_id);
        let unresolved_before_void = registry.ensure_resolved(None, "mass status");
        registry.observe_void_applied(&applied_void(voided_fill(&failed)));

        assert_eq!(voided_trade_ids(&failed), vec![observed.trade_id]);
        assert_eq!(apply_count(&failed), 0);
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::RestFailed)
        );
        assert_eq!(void_pending, Some(LegApplication::VoidPending));
        assert!(unresolved_before_void.is_err());
        assert_eq!(
            leg_application(&registry, &observed.trade_id),
            Some(LegApplication::VoidObserved)
        );
        assert_eq!(
            leg_application(&registry, &absent.trade_id),
            Some(LegApplication::Absent)
        );
        assert!(registry.ensure_resolved(None, "mass status").is_ok());
        assert!(registry.take_account_refresh());
    }

    #[rstest]
    #[case(PolymarketTradeStatus::Matched)]
    #[case(PolymarketTradeStatus::MatchedNotBroadcasted)]
    #[case(PolymarketTradeStatus::Mined)]
    #[case(PolymarketTradeStatus::Retrying)]
    fn test_non_terminal_rest_result_leaves_trade_quarantined(
        #[case] status: PolymarketTradeStatus,
    ) {
        let registry = live_registry();
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![taker_leg()]));

        let actions = registry.admit_rest_result(&trade(status, vec![taker_leg()]));

        assert!(actions.is_empty());
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::Quarantined)
        );
        assert_eq!(registry.pending_resolutions(), vec![TRADE.to_string()]);
        assert!(!registry.take_account_refresh());
    }

    #[rstest]
    #[case::economics_changed(vec![
        AdmittedLeg {
            last_qty: Quantity::from("9.00"),
            ..maker_leg("0xmaker-a")
        },
        maker_leg("0xmaker-b"),
    ])]
    #[case::owned_leg_missing(vec![maker_leg("0xmaker-a")])]
    fn test_contradicting_stream_evidence_quarantines(#[case] replay_legs: Vec<AdmittedLeg>) {
        let registry = live_registry();
        let legs = vec![maker_leg("0xmaker-a"), maker_leg("0xmaker-b")];
        for leg in &legs {
            registry.note_order_submitted(leg.venue_order_id);
        }

        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, legs));
        let woken_before_replay = resolution_woken(&registry);

        let replay = registry.admit_stream_trade(&trade(PolymarketTradeStatus::Mined, replay_legs));

        assert!(!woken_before_replay);
        assert_eq!(apply_count(&replay), 0);
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::Quarantined)
        );
        assert!(resolution_woken(&registry));
    }

    #[rstest]
    fn test_later_conflicting_rest_result_hard_faults() {
        let registry = live_registry();
        let leg = taker_leg();
        registry.quarantine_invalid_trade(TRADE);
        registry.admit_rest_result(&trade(PolymarketTradeStatus::Confirmed, vec![leg.clone()]));

        let identical =
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Confirmed, vec![leg.clone()]));
        let identical_fault = trade_hard_fault(&registry, TRADE);
        let conflicting =
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Failed, vec![leg]));

        assert!(identical.is_empty());
        assert!(identical_fault.is_none());
        assert!(conflicting.is_empty());
        assert!(trade_hard_fault(&registry, TRADE).is_some());
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::RestConfirmed)
        );
        assert!(registry.ensure_resolved(None, "mass status").is_err());
    }

    #[rstest]
    fn test_stale_stream_evidence_after_rest_failed_refreshes_without_effects() {
        let registry = live_registry();
        let leg = taker_leg();
        registry.quarantine_invalid_trade(TRADE);
        registry.admit_rest_result(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));
        registry.resolution_requested().now_or_never();

        let stale =
            registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![leg.clone()]));
        let pending = registry.pending_resolutions();
        registry.admit_rest_result(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));

        assert_eq!(apply_count(&stale), 0);
        assert!(resolution_woken(&registry));
        assert_eq!(pending, vec![TRADE.to_string()]);
        assert!(registry.pending_resolutions().is_empty());
        assert!(trade_hard_fault(&registry, TRADE).is_none());
        assert_eq!(
            leg_application(&registry, &leg.trade_id),
            Some(LegApplication::Absent)
        );
    }

    #[rstest]
    fn test_rest_confirmed_applies_quarantined_leg_with_rest_economics() {
        let registry = live_registry();
        let stream_leg = taker_leg();
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![stream_leg]));
        let mut rest_leg = taker_leg();
        rest_leg.last_qty = Quantity::from("9.00");

        let confirmed =
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Confirmed, vec![rest_leg]));

        let applied: Vec<Quantity> = confirmed
            .iter()
            .filter_map(|action| match action {
                SettlementAction::ApplyLeg { leg, .. } => Some(leg.last_qty),
                _ => None,
            })
            .collect();

        assert_eq!(applied, vec![Quantity::from("9.00")]);
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::RestConfirmed)
        );
        assert!(registry.pending_resolutions().is_empty());
    }

    #[rstest]
    fn test_buffered_fill_refused_after_quarantine_is_reapplied_by_rest_confirmed() {
        let registry = live_registry();
        let leg = taker_leg();
        registry.note_order_submitted(leg.venue_order_id);
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![leg.clone()]));
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));

        let claimed = registry.claim_buffered_fill(TRADE, &leg.trade_id);
        let confirmed =
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Confirmed, vec![leg.clone()]));
        let reclaimed = registry.claim_buffered_fill(TRADE, &leg.trade_id);

        assert!(!claimed);
        assert_eq!(apply_count(&confirmed), 1);
        assert!(reclaimed);
    }

    #[rstest]
    fn test_buffered_fill_drains_after_rest_confirmed_without_reapplication() {
        let registry = live_registry();
        let leg = taker_leg();
        registry.note_order_submitted(leg.venue_order_id);
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![leg.clone()]));
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));

        let confirmed =
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Confirmed, vec![leg.clone()]));
        let claimed = registry.claim_buffered_fill(TRADE, &leg.trade_id);

        assert_eq!(apply_count(&confirmed), 0);
        assert!(claimed);
    }

    #[rstest]
    fn test_buffered_fill_after_rest_failed_is_refused_as_tombstone() {
        let registry = live_registry();
        let leg = taker_leg();
        registry.note_order_submitted(leg.venue_order_id);
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![leg.clone()]));
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));
        registry.admit_rest_result(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));

        let claimed = registry.claim_buffered_fill(TRADE, &leg.trade_id);

        assert!(!claimed);
        assert_eq!(
            leg_application(&registry, &leg.trade_id),
            Some(LegApplication::Absent)
        );
        assert!(registry.ensure_resolved(None, "mass status").is_ok());
    }

    #[rstest]
    #[case::provisional(false, true)]
    #[case::rest_failed(true, false)]
    fn test_declined_fill_outcome(#[case] rest_failed_first: bool, #[case] faults: bool) {
        let registry = live_registry();
        let leg = taker_leg();
        registry.note_order_submitted(leg.venue_order_id);
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![leg.clone()]));
        registry.note_leg_enqueued(&leg.trade_id);

        if rest_failed_first {
            registry.admit_stream_trade(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));
        }

        registry.observe_fill_declined(&OrderEventAny::Filled(applied_fill(&leg, Some(TRADE))));
        let replay = registry
            .admit_stream_trade(&trade(PolymarketTradeStatus::Confirmed, vec![leg.clone()]));

        assert_eq!(trade_hard_fault(&registry, TRADE).is_some(), faults);
        assert_eq!(
            leg_application(&registry, &leg.trade_id),
            Some(LegApplication::Absent)
        );
        assert_eq!(apply_count(&replay), 0);
    }

    #[rstest]
    fn test_declined_void_hard_faults_trade() {
        let registry = live_registry();
        let leg = applied_taker(&registry);
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));
        let failed =
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));
        let voided = applied_void(voided_fill(&failed));

        registry.observe_fill_declined(&OrderEventAny::FillVoided(voided));

        assert!(trade_hard_fault(&registry, TRADE).is_some());
        assert_eq!(
            leg_application(&registry, &leg.trade_id),
            Some(LegApplication::FillObserved)
        );
    }

    #[rstest]
    fn test_late_fill_after_rest_failed_receives_one_void() {
        let registry = live_registry();
        let leg = taker_leg();
        registry.note_order_submitted(leg.venue_order_id);
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![leg.clone()]));
        registry.note_leg_enqueued(&leg.trade_id);
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));
        let failed =
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));
        let fill = applied_fill(&leg, Some(TRADE));

        let first = registry.observe_fill_applied(&fill);
        let duplicate = registry.observe_fill_applied(&fill);

        assert!(voided_trade_ids(&failed).is_empty());
        assert_eq!(
            first.map(|(key, fill)| (key, fill.trade_id)),
            Some((TRADE.to_string(), leg.trade_id))
        );
        assert!(duplicate.is_none());
    }

    #[rstest]
    fn test_unbound_maker_fill_binds_to_trade_evidence() {
        let registry = live_registry();
        let leg = maker_leg("0xmaker-a");
        registry.observe_fill_applied(&applied_fill(&leg, None));
        let unbound = leg_application(&registry, &leg.trade_id);
        let record_count = registry.record_count();

        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));
        let failed =
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));

        assert_eq!(unbound, Some(LegApplication::FillObserved));
        assert_eq!(record_count, 0);
        assert_eq!(voided_trade_ids(&failed), vec![leg.trade_id]);
        assert_eq!(registry.record_count(), 1);
    }

    #[rstest]
    fn test_unbound_maker_fill_observed_during_quarantine_binds_to_rest_evidence() {
        let registry = live_registry();
        let leg = maker_leg("0xmaker-a");
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));
        registry.observe_fill_applied(&applied_fill(&leg, None));

        let failed =
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));

        assert_eq!(voided_trade_ids(&failed), vec![leg.trade_id]);
        assert_eq!(
            leg_application(&registry, &leg.trade_id),
            Some(LegApplication::VoidPending)
        );
    }

    #[rstest]
    fn test_rest_confirmed_contradicting_applied_fill_hard_faults() {
        let registry = live_registry();
        let leg = applied_taker(&registry);
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));

        let rest_leg = AdmittedLeg {
            last_qty: Quantity::from("9.00"),
            ..leg
        };

        let confirmed =
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Confirmed, vec![rest_leg]));

        assert!(confirmed.is_empty());
        assert!(trade_hard_fault(&registry, TRADE).is_some());
        assert_eq!(
            leg_application(&registry, &leg.trade_id),
            Some(LegApplication::FillObserved)
        );
        assert!(registry.ensure_resolved(None, "mass status").is_err());
    }

    #[rstest]
    #[case::rest_confirmed(PolymarketTradeStatus::Confirmed, SettlementState::RestConfirmed, true)]
    // The owed void keeps reports blocked until core applies it
    #[case::rest_failed(PolymarketTradeStatus::Failed, SettlementState::RestFailed, false)]
    fn test_provisional_trade_requests_rest_resolution_after_session_change(
        #[case] rest_status: PolymarketTradeStatus,
        #[case] expected_state: SettlementState,
        #[case] expected_gate_open_after_rest: bool,
    ) {
        let registry = live_registry();
        let leg = applied_taker(&registry);
        let pending_before = registry.pending_resolutions();
        let gate_before = registry.ensure_resolved(None, "mass status");

        registry.begin_session();
        let woken = resolution_woken(&registry);
        let pending_after = registry.pending_resolutions();
        let gate_awaiting = registry.ensure_resolved(None, "mass status");
        let actions = registry.admit_rest_result(&trade(rest_status, vec![leg.clone()]));
        let gate_after_rest = registry.ensure_resolved(None, "mass status");

        let expected_voids = if rest_status == PolymarketTradeStatus::Failed {
            vec![leg.trade_id]
        } else {
            Vec::new()
        };

        assert!(pending_before.is_empty());
        assert!(gate_before.is_ok());
        assert!(woken);
        assert_eq!(pending_after, vec![TRADE.to_string()]);
        assert_eq!(
            gate_awaiting.unwrap_err().to_string(),
            "cannot generate mass status: Polymarket settlement registry holds 1 record(s) with \
             unresolved evidence"
        );
        assert_eq!(apply_count(&actions), 0);
        assert_eq!(voided_trade_ids(&actions), expected_voids);
        assert_eq!(settlement_state(&registry, TRADE), Some(expected_state));
        assert!(registry.pending_resolutions().is_empty());
        assert!(registry.take_account_refresh());
        assert_eq!(gate_after_rest.is_ok(), expected_gate_open_after_rest);
    }

    #[rstest]
    #[case::stream_confirmed(false)]
    #[case::hydrated(true)]
    fn test_session_change_skips_settled_and_hydrated_trades(#[case] hydrated: bool) {
        let registry = live_registry();
        let leg = taker_leg();

        if hydrated {
            registry.hydrate_fill(&applied_fill(&leg, Some(TRADE)));
        } else {
            applied_taker(&registry);
            registry.admit_stream_trade(&trade(PolymarketTradeStatus::Confirmed, vec![leg]));
        }

        registry.begin_session();

        assert!(!resolution_woken(&registry));
        assert!(registry.pending_resolutions().is_empty());
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(if hydrated {
                SettlementState::Provisional
            } else {
                SettlementState::StreamConfirmed
            })
        );
    }

    #[rstest]
    fn test_uncertain_order_gates_reports_in_scope_until_resolved() {
        let registry = live_registry();
        let venue_order_id = VenueOrderId::from("0xuncertain");
        let instrument_id = InstrumentId::from("TOKEN-A.POLYMARKET");

        registry.note_order_uncertain(venue_order_id, instrument_id, UnixNanos::from(7_u64));
        let woken = resolution_woken(&registry);
        let in_scope = registry.ensure_resolved(Some(instrument_id), "fill reports");
        let account_wide = registry.ensure_resolved(None, "mass status");
        let other_instrument = registry.ensure_resolved(
            Some(InstrumentId::from("TOKEN-B.POLYMARKET")),
            "fill reports",
        );
        let order_scope = registry.ensure_order_resolved(&venue_order_id, "fill reports");
        let other_order =
            registry.ensure_order_resolved(&VenueOrderId::from("0xother"), "fill reports");
        registry.begin_session();
        let rearmed = resolution_woken(&registry);
        let listed = registry.uncertain_orders();
        registry.clear_uncertain_order(&venue_order_id);

        assert!(woken);
        assert_eq!(
            in_scope.unwrap_err().to_string(),
            "cannot generate fill reports: 1 Polymarket order(s) have an unknown submit outcome"
        );
        assert_eq!(
            account_wide.unwrap_err().to_string(),
            "cannot generate mass status: 1 Polymarket order(s) have an unknown submit outcome"
        );
        assert!(other_instrument.is_ok());
        assert_eq!(
            order_scope.unwrap_err().to_string(),
            "cannot generate fill reports for venue order 0xuncertain: 1 Polymarket order(s) \
             have an unknown submit outcome"
        );
        assert!(other_order.is_ok());
        assert!(rearmed);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].0, venue_order_id);
        assert_eq!(listed[0].1.instrument_id, instrument_id);
        assert_eq!(listed[0].1.noted_at, UnixNanos::from(7_u64));
        assert!(registry.uncertain_orders().is_empty());
        assert!(registry.ensure_resolved(None, "mass status").is_ok());
    }

    #[rstest]
    #[case::pending(false)]
    #[case::closed(true)]
    fn test_repeated_uncertainty_preserves_terminal_recovery(#[case] closed: bool) {
        let registry = live_registry();
        let venue_order_id = VenueOrderId::from("0xuncertain");
        let instrument_id = InstrumentId::from("TOKEN-A.POLYMARKET");
        registry.note_order_uncertain(venue_order_id, instrument_id, UnixNanos::from(7_u64));

        if closed {
            registry.note_uncertain_submit_closed(&venue_order_id);
        }

        registry.note_order_uncertain(venue_order_id, instrument_id, UnixNanos::from(19_u64));
        let listed = registry.uncertain_orders();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].0, venue_order_id);
        assert_eq!(listed[0].1.instrument_id, instrument_id);
        assert_eq!(
            listed[0].1.noted_at,
            UnixNanos::from(if closed { 7_u64 } else { 19_u64 })
        );
        assert_eq!(
            listed[0].1.kind,
            if closed {
                UncertainOrderKind::SubmitClosed
            } else {
                UncertainOrderKind::Submit
            }
        );
    }

    #[rstest]
    fn test_stream_gap_reads_open_orders_and_gates_reports_in_scope() {
        let registry = live_registry();
        let instrument_a = InstrumentId::from("TOKEN-A.POLYMARKET");
        let instrument_b = InstrumentId::from("TOKEN-B.POLYMARKET");
        registry.note_order_open(
            ClientOrderId::from("O-1"),
            VenueOrderId::from("0xreplaced"),
            instrument_a,
        );
        registry.note_order_open(
            ClientOrderId::from("O-1"),
            VenueOrderId::from("0xopen"),
            instrument_a,
        );
        registry.note_order_open(
            ClientOrderId::from("O-2"),
            VenueOrderId::from("0xclosed"),
            instrument_b,
        );
        registry.note_order_closed(&ClientOrderId::from("O-2"));

        registry.begin_session();
        let woken_by_session = resolution_woken(&registry);
        registry.note_stream_gap(UnixNanos::from(7_u64));
        let woken = resolution_woken(&registry);

        let reads: Vec<_> = registry
            .uncertain_orders()
            .into_iter()
            .map(|(venue_order_id, order)| {
                (
                    venue_order_id,
                    order.instrument_id,
                    order.noted_at,
                    order.kind,
                )
            })
            .collect();

        let in_scope = registry.ensure_resolved(Some(instrument_a), "fill reports");
        let other_instrument = registry.ensure_resolved(Some(instrument_b), "fill reports");
        let order_scope =
            registry.ensure_order_resolved(&VenueOrderId::from("0xopen"), "fill reports");
        registry.clear_uncertain_order(&VenueOrderId::from("0xopen"));

        assert!(!woken_by_session);
        assert!(woken);
        assert_eq!(
            reads,
            vec![(
                VenueOrderId::from("0xopen"),
                instrument_a,
                UnixNanos::from(7_u64),
                UncertainOrderKind::StreamGap,
            )]
        );
        assert_eq!(
            in_scope.unwrap_err().to_string(),
            "cannot generate fill reports: 1 Polymarket order(s) await a trade read after a user \
             stream reconnect"
        );
        assert!(other_instrument.is_ok());
        assert_eq!(
            order_scope.unwrap_err().to_string(),
            "cannot generate fill reports for venue order 0xopen: 1 Polymarket order(s) await a \
             trade read after a user stream reconnect"
        );
        assert!(registry.ensure_resolved(None, "mass status").is_ok());
    }

    #[rstest]
    fn test_stream_gap_keeps_unknown_submit_read() {
        let registry = live_registry();
        let venue_order_id = VenueOrderId::from("0xuncertain");
        let instrument_id = InstrumentId::from("TOKEN-A.POLYMARKET");
        registry.note_order_uncertain(venue_order_id, instrument_id, UnixNanos::from(3_u64));
        registry.note_order_open(ClientOrderId::from("O-1"), venue_order_id, instrument_id);

        registry.note_stream_gap(UnixNanos::from(7_u64));
        let reads = registry.uncertain_orders();

        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].0, venue_order_id);
        assert_eq!(reads[0].1.noted_at, UnixNanos::from(3_u64));
        assert_eq!(reads[0].1.kind, UncertainOrderKind::Submit);
        assert_eq!(
            registry
                .ensure_resolved(None, "mass status")
                .unwrap_err()
                .to_string(),
            "cannot generate mass status: 1 Polymarket order(s) have an unknown submit outcome"
        );
    }

    #[rstest]
    fn test_knows_trade_from_evidence_or_unbound_observed_fill() {
        let registry = live_registry();
        let taker = taker_leg();
        let maker = maker_leg("0xmaker");
        let taker_trade = trade(PolymarketTradeStatus::Confirmed, vec![taker.clone()]);

        let maker_trade = AdmittedTrade {
            venue_trade_id: "trade-2".to_string(),
            status: PolymarketTradeStatus::Confirmed,
            legs: vec![maker.clone()],
        };

        let unknown = (
            registry.knows_trade(&taker_trade),
            registry.knows_trade(&maker_trade),
        );
        registry.note_order_submitted(taker.venue_order_id);
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![taker]));
        registry.observe_fill_applied(&applied_fill(&maker, None));

        assert_eq!(unknown, (false, false));
        assert!(registry.knows_trade(&taker_trade));
        assert!(registry.knows_trade(&maker_trade));
    }

    #[rstest]
    fn test_buffered_provisional_fill_from_earlier_session_waits_for_rest() {
        let registry = live_registry();
        let leg = taker_leg();
        registry.note_order_submitted(leg.venue_order_id);
        let authorized =
            registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![leg.clone()]));

        registry.begin_session();
        let claimed = registry.claim_buffered_fill(TRADE, &leg.trade_id);
        let confirmed =
            registry.admit_rest_result(&trade(PolymarketTradeStatus::Confirmed, vec![leg]));

        assert_eq!(apply_count(&authorized), 1);
        assert!(!claimed);
        assert_eq!(apply_count(&confirmed), 1);
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::RestConfirmed)
        );
    }

    #[rstest]
    fn test_hydrated_fill_is_not_reapplied_after_restart() {
        let registry = SettlementRegistry::new(AccountId::from("POLYMARKET-001"));
        let leg = taker_leg();
        registry.mark_hydrating();
        let hydrating = registry.ensure_resolved(None, "mass status");
        registry.hydrate_fill(&applied_fill(&leg, Some(TRADE)));
        registry.begin_session();
        registry.mark_live();

        let confirmed =
            registry.admit_stream_trade(&trade(PolymarketTradeStatus::Confirmed, vec![leg]));

        assert_eq!(
            hydrating.unwrap_err().to_string(),
            "cannot generate mass status: Polymarket settlement registry is hydrating"
        );
        assert!(confirmed.is_empty());
        assert!(registry.is_trade_confirmed(TRADE));
        assert!(registry.ensure_resolved(None, "mass status").is_ok());
    }

    #[rstest]
    fn test_rest_result_missing_applied_leg_hard_faults() {
        let registry = live_registry();
        let observed = maker_leg("0xmaker-a");
        let other = maker_leg("0xmaker-b");
        registry.observe_fill_applied(&applied_fill(&observed, Some(TRADE)));
        registry.quarantine_invalid_trade(TRADE);

        let failed = registry.admit_rest_result(&trade(PolymarketTradeStatus::Failed, vec![other]));

        assert!(failed.is_empty());
        assert!(trade_hard_fault(&registry, TRADE).is_some());
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::Quarantined)
        );
    }

    #[rstest]
    fn test_hard_faulted_trade_admits_no_new_effects() {
        let registry = live_registry();
        let leg = taker_leg();
        registry.note_order_submitted(leg.venue_order_id);
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![leg.clone()]));
        registry.note_leg_enqueued(&leg.trade_id);
        registry.observe_fill_declined(&OrderEventAny::Filled(applied_fill(&leg, Some(TRADE))));

        let stream =
            registry.admit_stream_trade(&trade(PolymarketTradeStatus::Failed, vec![leg.clone()]));
        let rest = registry.admit_rest_result(&trade(PolymarketTradeStatus::Confirmed, vec![leg]));

        assert!(stream.is_empty());
        assert!(rest.is_empty());
        assert!(registry.pending_resolutions().is_empty());
        assert!(registry.take_account_refresh());
    }

    #[rstest]
    fn test_invalid_evidence_for_rest_settled_trade_requests_refresh() {
        let registry = live_registry();
        registry.quarantine_invalid_trade(TRADE);
        registry.admit_rest_result(&trade(PolymarketTradeStatus::Confirmed, vec![taker_leg()]));
        registry.resolution_requested().now_or_never();

        registry.quarantine_invalid_trade(TRADE);

        assert!(resolution_woken(&registry));
        assert_eq!(
            settlement_state(&registry, TRADE),
            Some(SettlementState::RestConfirmed)
        );
        assert_eq!(registry.pending_resolutions(), vec![TRADE.to_string()]);
    }

    #[rstest]
    fn test_unresolved_evidence_blocks_only_reports_in_scope() {
        let registry = live_registry();
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![taker_leg()]));

        let in_scope = registry.ensure_resolved(
            Some(InstrumentId::from("TOKEN-A.POLYMARKET")),
            "order status reports",
        );
        let out_of_scope = registry.ensure_resolved(
            Some(InstrumentId::from("TOKEN-B.POLYMARKET")),
            "order status reports",
        );
        let order = registry.ensure_order_resolved(&VenueOrderId::from("0xtaker"), "fill reports");
        let other_order =
            registry.ensure_order_resolved(&VenueOrderId::from("0xother"), "fill reports");

        assert_eq!(
            in_scope.unwrap_err().to_string(),
            "cannot generate order status reports: Polymarket settlement registry holds 1 \
             record(s) with unresolved evidence"
        );
        assert!(out_of_scope.is_ok());
        assert_eq!(
            order.unwrap_err().to_string(),
            "cannot generate fill reports for venue order 0xtaker: Polymarket settlement \
             registry holds 1 record(s) with unresolved evidence"
        );
        assert!(other_order.is_ok());
    }

    #[rstest]
    fn test_report_routed_leg_is_not_pending() {
        let registry = live_registry();
        let leg = taker_leg();
        registry.note_order_submitted(leg.venue_order_id);
        registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![leg.clone()]));

        let buffered = registry.ensure_resolved(None, "mass status");
        registry.note_leg_reported(&leg.trade_id);

        assert!(buffered.is_err());
        assert!(registry.ensure_resolved(None, "mass status").is_ok());
    }

    #[rstest]
    fn test_capacity_exhaustion_after_hydration_faults_client() {
        let registry = SettlementRegistry::new(AccountId::from("POLYMARKET-001"));
        registry.mark_hydrating();

        for hydrated in ["hydrated-1", "hydrated-2"] {
            let leg = admitted_leg("0xhydrated", hydrated, LiquiditySide::Taker);
            registry.hydrate_fill(&applied_fill(&leg, None));
        }

        registry.mark_live();

        for index in 0..MAX_SETTLEMENT_RECORDS {
            registry.quarantine_invalid_trade(&format!("live-{index}"));
        }

        let at_capacity = registry.client_faulted();

        registry.quarantine_invalid_trade("overflow");
        let after_fault =
            registry.admit_stream_trade(&trade(PolymarketTradeStatus::Matched, vec![taker_leg()]));

        assert!(!at_capacity);
        assert!(after_fault.is_empty());
        assert!(registry.client_faulted());
        assert_eq!(registry.record_count(), MAX_SETTLEMENT_RECORDS + 2);
    }
}
