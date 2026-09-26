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

use ahash::AHashMap;
use nautilus_common::cache::fifo::FifoCache;
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::LiquiditySide,
    events::{OrderEventAny, OrderFillVoided, OrderFilled},
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    types::Money,
};
use parking_lot::Mutex;
use ustr::Ustr;

use super::{
    admission::{AdmittedLeg, AdmittedTrade},
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
        self.inner.lock().uncertain_orders.insert(
            venue_order_id,
            UncertainOrder {
                instrument_id,
                noted_at,
                kind: UncertainOrderKind::Submit,
            },
        );

        self.resolution_wakeup.notify_one();
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

        if let Some(record) = inner.records.get_mut(venue_trade_id)
            && record.hard_fault.is_none()
        {
            if record.settlement.is_rest_terminal() {
                record.refresh_requested = true;
            } else {
                enter_quarantined(record);
            }
        }

        self.wake_if_awaiting_resolution(&inner, venue_trade_id);
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
                    UncertainOrderKind::Submit => (submits + 1, stream_gaps),
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
            // Terminal REST economics are authoritative for a leg never sent to core
            *leg = SettlementLeg::from_admitted(incoming);
            leg.authorized = true;
            actions.push(SettlementAction::ApplyLeg {
                venue_trade_id: key.to_string(),
                leg: incoming.clone(),
            });
        } else if (leg.application != LegApplication::Absent || leg.authorized)
            && admitted_economics_diverge(leg, incoming)
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
        if stored.report_routed || stored.applied_fill.is_some() {
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
    previous.len() == incoming.len() && previous.iter().all(|leg| incoming.contains(leg))
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
        ts_event: fill.ts_event,
        application: LegApplication::FillObserved,
        applied_fill: Some(Box::new(fill.clone())),
        authorized: false,
        report_routed: false,
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
    use nautilus_model::{
        enums::{OrderSide, OrderType},
        identifiers::{StrategyId, TraderId},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::execution::get_pusd_currency;

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
