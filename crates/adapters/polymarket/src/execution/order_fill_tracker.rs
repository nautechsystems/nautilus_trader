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

//! Per-order fill tracking with terminal quantity normalization for the Polymarket adapter.

use std::sync::Mutex;

use ahash::{AHashMap, AHashSet};
use indexmap::IndexMap;
use nautilus_common::cache::fifo::{FifoCache, FifoCacheMap};
use nautilus_core::{MUTEX_POISONED, UnixNanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus},
    events::OrderFilled,
    identifiers::{ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    reports::{FillReport, OrderStatusReport},
    types::{Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::identity::OrderIdentity;
use crate::common::consts::DUST_SNAP_THRESHOLD_DEC;

/// Cumulative fill state for a single order.
#[derive(Debug, Clone)]
struct OrderFillState {
    submitted_qty: Quantity,
    cumulative_filled: Quantity,
    cumulative_quote_notional: Decimal,
    applied_fills: AHashMap<TradeId, FillFingerprint>,
    growth_policy: FillGrowthPolicy,
}

/// Authority-bearing fields which must remain identical for a replay of one venue trade ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FillFingerprint {
    instrument_id: InstrumentId,
    venue_order_id: VenueOrderId,
    trade_id: TradeId,
    order_side: OrderSide,
    last_qty: Quantity,
    last_px: Price,
    commission: Option<Money>,
    liquidity_side: LiquiditySide,
    ts_event: UnixNanos,
}

impl FillFingerprint {
    pub(crate) fn from_report(report: &FillReport) -> Self {
        Self {
            instrument_id: report.instrument_id,
            venue_order_id: report.venue_order_id,
            trade_id: report.trade_id,
            order_side: report.order_side,
            last_qty: report.last_qty,
            last_px: report.last_px,
            commission: Some(report.commission),
            liquidity_side: report.liquidity_side,
            ts_event: report.ts_event,
        }
    }

    pub(crate) fn from_event(fill: &OrderFilled) -> Self {
        Self {
            instrument_id: fill.instrument_id,
            venue_order_id: fill.venue_order_id,
            trade_id: fill.trade_id,
            order_side: fill.order_side,
            last_qty: fill.last_qty,
            last_px: fill.last_px,
            commission: fill.commission,
            liquidity_side: fill.liquidity_side,
            ts_event: fill.ts_event,
        }
    }

    pub(crate) fn ensure_equal(
        &self,
        other: &Self,
        venue_order_id: VenueOrderId,
    ) -> anyhow::Result<()> {
        let commission_equal = match (self.commission, other.commission) {
            (Some(expected), Some(received)) => {
                expected.currency == received.currency
                    && expected.as_decimal() == received.as_decimal()
            }
            (None, None) => true,
            _ => false,
        };
        let equal = self.instrument_id == other.instrument_id
            && self.venue_order_id == other.venue_order_id
            && self.trade_id == other.trade_id
            && self.order_side == other.order_side
            && self.last_qty.as_decimal() == other.last_qty.as_decimal()
            && self.last_px.as_decimal() == other.last_px.as_decimal()
            && commission_equal
            && self.liquidity_side == other.liquidity_side
            && self.ts_event == other.ts_event;
        anyhow::ensure!(
            equal,
            "trade {} replay carries different fill economics for order {venue_order_id}: expected {self:?}, received {other:?}",
            other.trade_id,
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum FillGrowthPolicy {
    #[default]
    Fixed,
    /// Durable order semantics prove an immediate quote BUY, but the exact signed quote budget is
    /// unavailable after restoration. Provider quantity is preserved, while any growth beyond
    /// the restored base quantity fails closed.
    QuoteImmediateBuyUnproven,
    /// A locally signed quote-quantity market BUY whose realized share quantity may exceed the
    /// pre-fill estimate, but whose exact fill notional cannot exceed the signed quote amount.
    QuoteImmediateBuy { signed_quote_budget: Decimal },
}

impl FillGrowthPolicy {
    pub(crate) fn quote_immediate_buy(signed_quote_budget: Decimal) -> Self {
        Self::QuoteImmediateBuy {
            signed_quote_budget,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FillCorrectionMetadata {
    pub correction_key: String,
    pub raw_trade_id: String,
    pub raw_corrective_timestamp: String,
    pub info: Option<IndexMap<Ustr, Ustr>>,
    pub is_confirmed: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct BufferedFill {
    pub report: FillReport,
    pub correction: Option<FillCorrectionMetadata>,
}

#[derive(Clone, Debug)]
pub(crate) struct BufferedFillEmission {
    pub buffered: BufferedFill,
    pub emitted: bool,
}

#[derive(Debug)]
pub(crate) struct FillBatchAdmission<T> {
    pub reports: Vec<Option<(FillReport, T, Option<Quantity>)>>,
    pub binding_error: Option<anyhow::Error>,
}

#[derive(Debug)]
pub(crate) enum PendingOrderReportDrain {
    Empty,
    WaitingForFill,
    Rejected(Vec<OrderStatusReport>),
    Registered(Vec<OrderStatusReport>),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CorrectionFillEvidence {
    pub pending: Vec<FillReport>,
    pub applied: Vec<OrderFilled>,
}

/// Registration map plus the fill and order-report buffers, all under one mutex.
///
/// Co-locating the buffers with the registration map is what closes the buffer-after-drain race:
/// the WS dispatch's accepted-check and buffer, and the submit path's register and drain, are all
/// single critical sections on this one lock, so a buffer can never slip between a register and the
/// drain that follows it.
#[derive(Debug, Default)]
struct TrackerInner {
    orders: AHashMap<VenueOrderId, OrderFillState>,
    pending_fills: FifoCacheMap<VenueOrderId, Vec<BufferedFill>, 1_000>,
    pending_reports: FifoCacheMap<VenueOrderId, Vec<OrderStatusReport>, 1_000>,
    voided_trades: FifoCache<String, 10_000>,
    applied_buffered_fills: FifoCacheMap<String, AppliedBufferedCorrection, 10_000>,
}

#[derive(Clone, Debug, Default)]
struct AppliedBufferedCorrection {
    fills: Vec<OrderFilled>,
    is_confirmed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProspectiveFillAdmission {
    New { quantity_update: Option<Quantity> },
    Replay,
}

/// Tracks per-order fill accumulation, detects dust residuals, and buffers WS messages that arrive
/// before the order is registered.
///
/// Thread-safe: a single internal `Mutex<TrackerInner>` -- safe to share via `Arc` across the WS
/// task and spawned order submission tasks. Because registration and buffering share that lock, the
/// accepted-or-buffer decision and the register-and-drain are mutually atomic.
#[derive(Debug)]
pub(crate) struct OrderFillTrackerMap {
    inner: Mutex<TrackerInner>,
}

impl OrderFillTrackerMap {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(TrackerInner::default()),
        }
    }

    pub(crate) fn restore_order(
        &self,
        venue_order_id: VenueOrderId,
        submitted_qty: Quantity,
        filled_qty: Quantity,
        growth_policy: FillGrowthPolicy,
        applied_fills: Vec<OrderFilled>,
    ) -> anyhow::Result<()> {
        let mut state = new_order_state(submitted_qty, growth_policy);
        state.cumulative_filled = filled_qty;
        for fill in applied_fills {
            anyhow::ensure!(
                fill.venue_order_id == venue_order_id,
                "restored fill {} belongs to order {}, expected {venue_order_id}",
                fill.trade_id,
                fill.venue_order_id,
            );
            let fingerprint = FillFingerprint::from_event(&fill);
            if let Some(existing) = state.applied_fills.get(&fill.trade_id) {
                existing.ensure_equal(&fingerprint, venue_order_id)?;
            } else {
                state.applied_fills.insert(fill.trade_id, fingerprint);
            }
        }
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .orders
            .insert(venue_order_id, state);
        Ok(())
    }

    /// Returns true if the order has been registered (accepted).
    pub(crate) fn contains(&self, venue_order_id: &VenueOrderId) -> bool {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .orders
            .get(venue_order_id)
            .is_some()
    }

    /// Returns true if the order has received any fills or been removed (settled).
    pub(crate) fn has_fills_or_settled(&self, venue_order_id: &VenueOrderId) -> bool {
        match self
            .inner
            .lock()
            .expect(MUTEX_POISONED)
            .orders
            .get(venue_order_id)
        {
            Some(s) => !s.cumulative_filled.is_zero(),
            None => true, // Removed = already settled
        }
    }

    /// Returns the cumulative filled quantity for an order, if tracked.
    pub(crate) fn get_cumulative_filled(&self, venue_order_id: &VenueOrderId) -> Option<Quantity> {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .orders
            .get(venue_order_id)
            .map(|s| s.cumulative_filled)
    }

    /// Returns the registered submitted quantity for an order, if tracked.
    pub(crate) fn submitted_qty(&self, venue_order_id: &VenueOrderId) -> Option<Quantity> {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .orders
            .get(venue_order_id)
            .map(|s| s.submitted_qty)
    }

    /// Validates returned fills together with already-applied tracked authority.
    pub(crate) fn validate_confirmed_fills(
        &self,
        cached_fills: &[OrderFilled],
        fill_reports: &[FillReport],
    ) -> anyhow::Result<AHashSet<VenueOrderId>> {
        let guard = self.inner.lock().expect(MUTEX_POISONED);
        let mut prospective_orders = AHashMap::new();
        let mut validated_orders = AHashSet::new();

        for fill in cached_fills {
            let Some(current) = guard.orders.get(&fill.venue_order_id) else {
                continue;
            };
            validated_orders.insert(fill.venue_order_id);
            let state = prospective_orders
                .entry(fill.venue_order_id)
                .or_insert_with(|| current.clone());
            validate_or_admit_fill_in(
                state,
                FillFingerprint::from_event(fill),
                &fill.venue_order_id,
            )?;
        }

        for report in fill_reports {
            let Some(current) = guard.orders.get(&report.venue_order_id) else {
                continue;
            };
            validated_orders.insert(report.venue_order_id);
            let state = prospective_orders
                .entry(report.venue_order_id)
                .or_insert_with(|| current.clone());
            validate_or_admit_fill_in(
                state,
                FillFingerprint::from_report(report),
                &report.venue_order_id,
            )?;
        }
        Ok(validated_orders)
    }

    /// Returns `true` if cumulative fills have reached the submitted quantity.
    pub(crate) fn is_fully_filled(&self, venue_order_id: &VenueOrderId) -> bool {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .orders
            .get(venue_order_id)
            .is_some_and(|s| s.cumulative_filled >= s.submitted_qty)
    }

    /// Records a tracked fill, or buffers it until the order is registered, atomically.
    ///
    /// The accepted-check and the buffer insert run under one lock, so the submit path's register
    /// and drain (the same lock) cannot interleave between them. Returns the report only when the
    /// order is registered and the caller proves it can emit a reversible order event; otherwise
    /// the report remains buffered.
    #[cfg(test)]
    pub(crate) fn accept_or_buffer_fill<F>(
        &self,
        venue_order_id: VenueOrderId,
        report: FillReport,
        correction: FillCorrectionMetadata,
        mut reversible_target: F,
    ) -> anyhow::Result<Option<FillReport>>
    where
        F: FnMut(&FillReport) -> anyhow::Result<bool>,
    {
        let mut outcome = self
            .accept_or_buffer_fills(vec![(venue_order_id, report, correction)], |report| {
                reversible_target(report).map(|can_emit| can_emit.then_some(()))
            })?;

        if let Some(e) = outcome.binding_error {
            return Err(e);
        }
        Ok(outcome
            .reports
            .pop()
            .flatten()
            .map(|(report, (), _)| report))
    }

    /// Admits one native correction batch only after every participant binding is checked.
    ///
    /// A structurally invalid batch is rejected without retention. A target-binding error retains
    /// the entire untouched batch. A replay matching already-pending correction participants is
    /// also retained without duplication, so the eventual registered order drain remains the only
    /// authority transition for that evidence.
    pub(crate) fn accept_or_buffer_fills<T, F>(
        &self,
        fills: Vec<(VenueOrderId, FillReport, FillCorrectionMetadata)>,
        mut reversible_target: F,
    ) -> anyhow::Result<FillBatchAdmission<T>>
    where
        F: FnMut(&FillReport) -> anyhow::Result<Option<T>>,
    {
        let report_count = fills.len();
        let mut participants =
            AHashSet::<(String, VenueOrderId, TradeId)>::with_capacity(fills.len());
        for (venue_order_id, report, correction) in &fills {
            let participant = (
                correction.correction_key.clone(),
                *venue_order_id,
                report.trade_id,
            );

            if !participants.insert(participant) {
                anyhow::bail!(
                    "duplicate correction participant {} for order {} and trade {}",
                    correction.correction_key,
                    venue_order_id,
                    report.trade_id
                );
            }
        }

        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        let already_pending = fills
            .iter()
            .map(|(_, report, correction)| {
                pending_correction_participant(&guard, report, correction)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        if already_pending.iter().any(|pending| *pending) {
            for (_, report, correction) in &fills {
                anyhow::ensure!(
                    existing_correction_participant(&guard, report, correction)?,
                    "partial correction replay {} has no pending or applied evidence for order {} and trade {}",
                    correction.correction_key,
                    report.venue_order_id,
                    report.trade_id,
                );
            }
            return Ok(FillBatchAdmission {
                reports: (0..report_count).map(|_| None).collect(),
                binding_error: None,
            });
        }

        let mut decisions = Vec::with_capacity(fills.len());
        for (_, report, _) in &fills {
            match reversible_target(report) {
                Ok(target) => decisions.push(target),
                Err(e) => {
                    let report_count = fills.len();
                    for (venue_order_id, report, correction) in fills {
                        push_buffered(
                            &mut guard.pending_fills,
                            venue_order_id,
                            BufferedFill {
                                report,
                                correction: Some(correction),
                            },
                        );
                    }
                    return Ok(FillBatchAdmission {
                        reports: (0..report_count).map(|_| None).collect(),
                        binding_error: Some(e),
                    });
                }
            }
        }

        let mut prospective_orders = AHashMap::new();
        let mut admissions = Vec::with_capacity(fills.len());
        for ((venue_order_id, report, _), target) in fills.iter().zip(&decisions) {
            let admission = if target.is_some() {
                guard.orders.get(venue_order_id).cloned().map(|current| {
                    let state = prospective_orders.entry(*venue_order_id).or_insert(current);
                    admit_fill_in(state, report, venue_order_id)
                })
            } else {
                None
            }
            .transpose()?;
            admissions.push(admission);
        }

        for (venue_order_id, state) in prospective_orders {
            guard.orders.insert(venue_order_id, state);
        }

        let reports = fills
            .into_iter()
            .zip(decisions)
            .zip(admissions)
            .map(
                |(((venue_order_id, report, correction), target), admission)| match (
                    target, admission,
                ) {
                    (Some(target), Some(ProspectiveFillAdmission::New { quantity_update })) => {
                        Some((report, target, quantity_update))
                    }
                    (Some(_), Some(ProspectiveFillAdmission::Replay)) => None,
                    _ => {
                        push_buffered(
                            &mut guard.pending_fills,
                            venue_order_id,
                            BufferedFill {
                                report,
                                correction: Some(correction),
                            },
                        );
                        None
                    }
                },
            )
            .collect();
        Ok(FillBatchAdmission {
            reports,
            binding_error: None,
        })
    }

    /// Registers an accepted order while retaining fills until reversible identity is available.
    pub(crate) fn register_without_draining(
        &self,
        venue_order_id: VenueOrderId,
        submitted_qty: Quantity,
        growth_policy: FillGrowthPolicy,
    ) {
        self.inner.lock().expect(MUTEX_POISONED).orders.insert(
            venue_order_id,
            new_order_state(submitted_qty, growth_policy),
        );
    }

    /// Returns a bound order report to process, or buffers it until binding becomes visible.
    ///
    /// The registration check, late binding check, and buffer insert run under one lock. Once the
    /// submit path has published the binding, a WS task which observed the earlier unbound state
    /// returns the report for validation instead of appending it behind a completed drain.
    pub(crate) fn accept_or_buffer_report<F>(
        &self,
        venue_order_id: VenueOrderId,
        report: OrderStatusReport,
        binding_visible: F,
    ) -> Option<(OrderStatusReport, bool)>
    where
        F: FnOnce() -> bool,
    {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        let registered = guard.orders.get(&venue_order_id).is_some();
        if registered || binding_visible() {
            Some((report, registered))
        } else {
            push_buffered(&mut guard.pending_reports, venue_order_id, report);
            None
        }
    }

    /// Retains an order report without treating registration as emission authority.
    pub(crate) fn buffer_report(&self, venue_order_id: VenueOrderId, report: OrderStatusReport) {
        push_buffered(
            &mut self.inner.lock().expect(MUTEX_POISONED).pending_reports,
            venue_order_id,
            report,
        );
    }

    /// Emits one known-order report only when no earlier fill is awaiting binding.
    ///
    /// The pending-fill check, report retention, and callback are one critical section. The
    /// callback must not call back into this tracker.
    pub(crate) fn emit_or_buffer_report_if_no_pending_fill<F>(
        &self,
        venue_order_id: VenueOrderId,
        report: OrderStatusReport,
        emit: F,
    ) -> bool
    where
        F: FnOnce(&OrderStatusReport),
    {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        if guard
            .pending_fills
            .get(&venue_order_id)
            .is_some_and(|fills| !fills.is_empty())
        {
            push_buffered(&mut guard.pending_reports, venue_order_id, report);
            return false;
        }
        emit(&report);
        true
    }

    /// Classifies and removes the exact retained report set for an ambiguous submit outcome.
    ///
    /// Filtering, pending-fill inspection, tracker registration, and removal are one critical
    /// section. A racing WS task therefore either contributes to this decision or observes the
    /// already-published identity and processes its report directly.
    pub(crate) fn classify_pending_order_reports<F>(
        &self,
        venue_order_id: VenueOrderId,
        submitted_qty: Quantity,
        growth_policy: FillGrowthPolicy,
        mut retain: F,
    ) -> PendingOrderReportDrain
    where
        F: FnMut(&OrderStatusReport) -> bool,
    {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        let remove_empty_entry = match guard.pending_reports.get_mut(&venue_order_id) {
            Some(reports) => {
                reports.retain(&mut retain);
                reports.is_empty()
            }
            None => false,
        };

        if remove_empty_entry {
            guard.pending_reports.remove(&venue_order_id);
        }

        if guard.orders.contains_key(&venue_order_id) {
            return PendingOrderReportDrain::Registered(
                guard
                    .pending_reports
                    .get(&venue_order_id)
                    .cloned()
                    .unwrap_or_default(),
            );
        }

        let Some(reports) = guard.pending_reports.get(&venue_order_id) else {
            return PendingOrderReportDrain::Empty;
        };

        if reports
            .iter()
            .all(|report| report.order_status == OrderStatus::Rejected)
        {
            if guard
                .pending_fills
                .get(&venue_order_id)
                .is_some_and(|fills| !fills.is_empty())
            {
                return PendingOrderReportDrain::WaitingForFill;
            }
            return PendingOrderReportDrain::Rejected(
                guard
                    .pending_reports
                    .remove(&venue_order_id)
                    .unwrap_or_default(),
            );
        }

        let reports = reports.clone();
        guard.orders.insert(
            venue_order_id,
            new_order_state(submitted_qty, growth_policy),
        );
        PendingOrderReportDrain::Registered(reports)
    }

    /// Emits bound buffered fills for an already-registered order under one tracker lock.
    pub(crate) fn emit_pending_fills_for_registered<B, F>(
        &self,
        venue_order_id: VenueOrderId,
        client_order_id: Option<ClientOrderId>,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        before_emit: B,
        emit: F,
    ) -> anyhow::Result<Vec<BufferedFillEmission>>
    where
        B: FnOnce(&[BufferedFill]),
        F: FnMut(&BufferedFill, Option<Quantity>) -> OrderFilled,
    {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        emit_pending_fills(
            &mut guard,
            venue_order_id,
            client_order_id,
            instrument_id,
            order_side,
            before_emit,
            emit,
        )
    }

    /// Registers and emits only when a buffered fill already proves venue acceptance.
    pub(crate) fn register_and_emit_pending_fills_if_buffered<B, F>(
        &self,
        venue_order_id: VenueOrderId,
        identity: OrderIdentity,
        submitted_qty: Quantity,
        growth_policy: FillGrowthPolicy,
        before_emit: B,
        emit: F,
    ) -> anyhow::Result<Option<Vec<BufferedFillEmission>>>
    where
        B: FnOnce(&[BufferedFill]),
        F: FnMut(&BufferedFill, Option<Quantity>) -> OrderFilled,
    {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        if !guard.pending_fills.contains_key(&venue_order_id) {
            return Ok(None);
        }
        validate_pending_fill_binding(
            &guard,
            venue_order_id,
            identity.instrument_id,
            identity.order_side,
        )?;
        guard.orders.insert(
            venue_order_id,
            new_order_state(submitted_qty, growth_policy),
        );
        Ok(Some(emit_pending_fills(
            &mut guard,
            venue_order_id,
            Some(identity.client_order_id),
            identity.instrument_id,
            identity.order_side,
            before_emit,
            emit,
        )?))
    }

    /// Drains buffered order reports for a registered order (raw, for conversion by the caller).
    pub(crate) fn take_pending_reports(
        &self,
        venue_order_id: &VenueOrderId,
    ) -> Vec<OrderStatusReport> {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .pending_reports
            .remove(venue_order_id)
            .unwrap_or_default()
    }

    /// Drops contradictory buffered reports and returns a snapshot of those retained.
    pub(crate) fn retain_pending_reports_for<F>(
        &self,
        venue_order_id: &VenueOrderId,
        mut retain: F,
    ) -> Vec<OrderStatusReport>
    where
        F: FnMut(&OrderStatusReport) -> bool,
    {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        let (snapshot, remove_entry) = match guard.pending_reports.get_mut(venue_order_id) {
            Some(reports) => {
                reports.retain(&mut retain);
                (reports.clone(), reports.is_empty())
            }
            None => return Vec::new(),
        };

        if remove_entry {
            guard.pending_reports.remove(venue_order_id);
        }
        snapshot
    }

    /// Snapshots all buffered and applied evidence for one correction under one lock.
    pub(crate) fn correction_fill_evidence(&self, correction_key: &str) -> CorrectionFillEvidence {
        let guard = self.inner.lock().expect(MUTEX_POISONED);
        let pending = guard
            .pending_fills
            .values()
            .flat_map(|fills| fills.iter())
            .filter(|fill| {
                fill.correction
                    .as_ref()
                    .is_some_and(|metadata| metadata.correction_key == correction_key)
            })
            .map(|fill| fill.report.clone())
            .collect();
        let applied = guard
            .applied_buffered_fills
            .get(&correction_key.to_string())
            .map(|correction| correction.fills.clone())
            .unwrap_or_default();
        CorrectionFillEvidence { pending, applied }
    }

    /// Promotes matching pending fill metadata after a CONFIRMED replay.
    ///
    /// This records no authority and does not mark the correction confirmed. Confirmation is
    /// recorded only if a later order-registration drain actually emits the buffered fill.
    pub(crate) fn promote_pending_trade_confirmed(
        &self,
        venue_order_ids: &[VenueOrderId],
        correction_key: &str,
        raw_trade_id: &str,
        raw_corrective_timestamp: &str,
    ) -> bool {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);

        for venue_order_id in venue_order_ids {
            promote_pending_correction(
                guard.pending_fills.get_mut(venue_order_id),
                correction_key,
                raw_trade_id,
                raw_corrective_timestamp,
            );
        }

        if let Some(applied) = guard
            .applied_buffered_fills
            .get_mut(&correction_key.to_string())
        {
            applied.is_confirmed = true;
            return !applied.fills.is_empty();
        }
        false
    }

    /// Marks a trade failed and returns buffered fills that were already emitted.
    pub(crate) fn void_buffered_trade(
        &self,
        correction_key: &str,
    ) -> anyhow::Result<Vec<OrderFilled>> {
        let key = correction_key.to_string();
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        anyhow::ensure!(
            !guard
                .applied_buffered_fills
                .get(&key)
                .is_some_and(|correction| correction.is_confirmed),
            "cannot void finalized correction {correction_key}"
        );
        guard.voided_trades.add(key.clone());
        let fills = guard
            .applied_buffered_fills
            .remove(&key)
            .map(|correction| correction.fills)
            .unwrap_or_default();

        for fill in &fills {
            reverse_fill_in(
                &mut guard.orders,
                &fill.venue_order_id,
                fill.trade_id,
                fill.last_px,
                fill.last_qty,
            );
        }
        Ok(fills)
    }

    pub(crate) fn reverse_fill(
        &self,
        venue_order_id: &VenueOrderId,
        trade_id: TradeId,
        last_px: Price,
        quantity: Quantity,
    ) {
        reverse_fill_in(
            &mut self.inner.lock().expect(MUTEX_POISONED).orders,
            venue_order_id,
            trade_id,
            last_px,
            quantity,
        );
    }

    /// Snap each report's `last_qty` against the registered submitted quantity
    /// for its `venue_order_id`. Reports for orders the tracker does not know
    /// about (e.g. orders from another session) pass through unchanged.
    ///
    /// Commission is intentionally not recomputed: it tracks the venue charge
    /// from the on-chain fill, which is independent of our local snap.
    pub(crate) fn snap_fill_reports(&self, reports: &mut [FillReport]) {
        let guard = self.inner.lock().expect(MUTEX_POISONED);

        for report in reports {
            report.last_qty =
                snap_fill_qty_in(&guard.orders, &report.venue_order_id, report.last_qty);
        }
    }

    /// Snap a single fill qty DOWN to `submitted_qty` when the venue reports
    /// dust overfill (within `DUST_SNAP_THRESHOLD_DEC`).
    ///
    /// Overfill snapping is required because the engine rejects fills past
    /// `submitted_qty`. Underfill is intentionally left alone here: a single
    /// partial fill that happens to land near submitted_qty might still be
    /// followed by additional matches, or the order might end up canceled
    /// with the dust remaining as legitimate leaves. Terminal quantity
    /// normalization handles the CLOB cent-tick truncation
    /// case after all associated trades confirm.
    ///
    /// See `docs/integrations/polymarket.md` (Fill quantity normalization).
    pub(crate) fn snap_fill_qty(
        &self,
        venue_order_id: &VenueOrderId,
        fill_qty: Quantity,
    ) -> Quantity {
        let guard = self.inner.lock().expect(MUTEX_POISONED);
        snap_fill_qty_in(&guard.orders, venue_order_id, fill_qty)
    }

    /// Returns the venue-filled quantity when a terminal order has sub-cent-share leaves.
    ///
    /// The returned quantity is used for an order-only reconciliation update. It is not a fill and
    /// must not change positions, balances, or commissions. The entry is removed on normalization
    /// so repeated terminal messages are idempotent.
    pub(crate) fn check_terminal_quantity_normalization(
        &self,
        venue_order_id: &VenueOrderId,
    ) -> Option<Quantity> {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        let s = guard.orders.get(venue_order_id)?;
        if s.cumulative_filled >= s.submitted_qty {
            return None;
        }
        let leaves = s.submitted_qty.as_decimal() - s.cumulative_filled.as_decimal();

        if leaves > Decimal::ZERO && leaves < DUST_SNAP_THRESHOLD_DEC {
            let filled_qty = s.cumulative_filled;

            log::debug!(
                "Normalizing terminal order {venue_order_id} quantity from {} to {filled_qty} \
                 (non-economic leaves={leaves})",
                s.submitted_qty,
            );
            guard.orders.remove(venue_order_id);
            Some(filled_qty)
        } else {
            if leaves >= DUST_SNAP_THRESHOLD_DEC {
                log::debug!(
                    "Order {venue_order_id} MATCHED with significant residual \
                     {leaves} (filled {}/{})",
                    s.cumulative_filled,
                    s.submitted_qty,
                );
            }
            None
        }
    }

    /// Returns the real unfilled remainder of a terminal IOC order.
    ///
    /// The entry is removed so duplicate `CONFIRMED` trade messages cannot emit repeated
    /// cancellations. The caller must use this only after a taker trade confirms: that proves the
    /// FAK order has finished matching and the venue has killed the returned remainder.
    pub(crate) fn take_terminal_ioc_remainder(
        &self,
        venue_order_id: &VenueOrderId,
    ) -> Option<Quantity> {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        let state = guard.orders.get(venue_order_id)?;
        if state.cumulative_filled.is_zero() || state.cumulative_filled >= state.submitted_qty {
            return None;
        }

        let remainder = state.submitted_qty - state.cumulative_filled;
        guard.orders.remove(venue_order_id);
        Some(remainder)
    }
}

fn new_order_state(submitted_qty: Quantity, growth_policy: FillGrowthPolicy) -> OrderFillState {
    OrderFillState {
        submitted_qty,
        cumulative_filled: Quantity::zero(submitted_qty.precision),
        cumulative_quote_notional: Decimal::ZERO,
        applied_fills: AHashMap::new(),
        growth_policy,
    }
}

fn pending_correction_participant(
    inner: &TrackerInner,
    report: &FillReport,
    correction: &FillCorrectionMetadata,
) -> anyhow::Result<bool> {
    let expected = FillFingerprint::from_report(report);
    for pending in inner.pending_fills.values().flat_map(|fills| fills.iter()) {
        if pending.report.venue_order_id == report.venue_order_id
            && pending.report.trade_id == report.trade_id
            && pending
                .correction
                .as_ref()
                .is_some_and(|metadata| metadata.correction_key == correction.correction_key)
        {
            FillFingerprint::from_report(&pending.report)
                .ensure_equal(&expected, report.venue_order_id)?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn existing_correction_participant(
    inner: &TrackerInner,
    report: &FillReport,
    correction: &FillCorrectionMetadata,
) -> anyhow::Result<bool> {
    if pending_correction_participant(inner, report, correction)? {
        return Ok(true);
    }

    let expected = FillFingerprint::from_report(report);
    if let Some(existing) = inner
        .orders
        .get(&report.venue_order_id)
        .and_then(|state| state.applied_fills.get(&report.trade_id))
    {
        existing.ensure_equal(&expected, report.venue_order_id)?;
        return Ok(true);
    }

    if let Some(existing) = inner
        .applied_buffered_fills
        .get(&correction.correction_key)
        .and_then(|applied| {
            applied.fills.iter().find(|fill| {
                fill.venue_order_id == report.venue_order_id && fill.trade_id == report.trade_id
            })
        })
    {
        FillFingerprint::from_event(existing).ensure_equal(&expected, report.venue_order_id)?;
        return Ok(true);
    }

    Ok(false)
}

fn admit_fill_in(
    state: &mut OrderFillState,
    report: &FillReport,
    venue_order_id: &VenueOrderId,
) -> anyhow::Result<ProspectiveFillAdmission> {
    let fingerprint = FillFingerprint::from_report(report);
    if let Some(existing) = state.applied_fills.get(&report.trade_id) {
        existing.ensure_equal(&fingerprint, *venue_order_id)?;
        return Ok(ProspectiveFillAdmission::Replay);
    }
    let quantity_update = admit_new_fill_in(state, fingerprint, venue_order_id)?;
    Ok(ProspectiveFillAdmission::New { quantity_update })
}

fn validate_or_admit_fill_in(
    state: &mut OrderFillState,
    fingerprint: FillFingerprint,
    venue_order_id: &VenueOrderId,
) -> anyhow::Result<()> {
    if let Some(existing) = state.applied_fills.get(&fingerprint.trade_id) {
        existing.ensure_equal(&fingerprint, *venue_order_id)?;
        return Ok(());
    }

    admit_new_fill_in(state, fingerprint, venue_order_id)?;
    Ok(())
}

fn admit_new_fill_in(
    state: &mut OrderFillState,
    fingerprint: FillFingerprint,
    venue_order_id: &VenueOrderId,
) -> anyhow::Result<Option<Quantity>> {
    let qty = fingerprint.last_qty;
    let px = fingerprint.last_px;
    let cumulative = state.cumulative_filled.checked_add(qty).ok_or_else(|| {
        anyhow::anyhow!(
            "fill quantity overflow for order {venue_order_id}: {} + {qty}",
            state.cumulative_filled
        )
    })?;
    let cumulative_quote_notional = match state.growth_policy {
        FillGrowthPolicy::Fixed | FillGrowthPolicy::QuoteImmediateBuyUnproven => {
            state.cumulative_quote_notional
        }
        FillGrowthPolicy::QuoteImmediateBuy {
            signed_quote_budget,
        } => {
            let fill_notional = qty
                .as_decimal()
                .checked_mul(px.as_decimal())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "fill quote notional overflow for order {venue_order_id}: {qty} * {px}",
                    )
                })?;
            let total = state
                .cumulative_quote_notional
                .checked_add(fill_notional)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "aggregate quote notional overflow for order {venue_order_id}: {} + {fill_notional}",
                        state.cumulative_quote_notional,
                    )
                })?;
            anyhow::ensure!(
                total <= signed_quote_budget,
                "aggregate quote notional {total} exceeds signed quote budget {signed_quote_budget} for order {venue_order_id}",
            );
            total
        }
    };
    let quantity_update = if cumulative <= state.submitted_qty {
        None
    } else {
        anyhow::ensure!(
            matches!(
                state.growth_policy,
                FillGrowthPolicy::QuoteImmediateBuy { .. }
            ),
            "aggregate fill {cumulative} exceeds submitted quantity {} for order {venue_order_id}; exact signed quote budget is unavailable",
            state.submitted_qty
        );
        Some(cumulative)
    };

    state.cumulative_filled = cumulative;
    state.cumulative_quote_notional = cumulative_quote_notional;
    if let Some(new_qty) = quantity_update {
        state.submitted_qty = new_qty;
    }
    state
        .applied_fills
        .insert(fingerprint.trade_id, fingerprint);
    Ok(quantity_update)
}

fn validate_pending_fill_binding(
    inner: &TrackerInner,
    venue_order_id: VenueOrderId,
    instrument_id: InstrumentId,
    order_side: OrderSide,
) -> anyhow::Result<()> {
    let Some(fills) = inner.pending_fills.get(&venue_order_id) else {
        return Ok(());
    };

    for fill in fills {
        anyhow::ensure!(
            fill.report.venue_order_id == venue_order_id,
            "buffered fill venue order {} does not match registered order {venue_order_id}",
            fill.report.venue_order_id
        );
        anyhow::ensure!(
            fill.report.instrument_id == instrument_id,
            "buffered fill instrument {} does not match registered instrument {instrument_id}",
            fill.report.instrument_id
        );
        anyhow::ensure!(
            fill.report.order_side == order_side,
            "buffered fill side {} does not match registered side {order_side}",
            fill.report.order_side
        );
    }
    Ok(())
}

/// Snapshots the buffered fills for `venue_order_id`, validates their eventual order binding, and
/// stamps the client order ID. Removal happens only after the complete quantity plan succeeds.
fn prepare_pending_fills(
    inner: &TrackerInner,
    venue_order_id: VenueOrderId,
    client_order_id: Option<ClientOrderId>,
    instrument_id: InstrumentId,
    order_side: OrderSide,
) -> anyhow::Result<Vec<BufferedFill>> {
    validate_pending_fill_binding(inner, venue_order_id, instrument_id, order_side)?;
    let Some(buffered) = inner.pending_fills.get(&venue_order_id).cloned() else {
        return Ok(Vec::new());
    };
    Ok(buffered
        .into_iter()
        .map(|mut buffered| {
            buffered.report.client_order_id = client_order_id;
            buffered.report.last_qty =
                snap_fill_qty_in(&inner.orders, &venue_order_id, buffered.report.last_qty);
            buffered
        })
        .collect())
}

fn emit_pending_fills<B, F>(
    inner: &mut TrackerInner,
    venue_order_id: VenueOrderId,
    client_order_id: Option<ClientOrderId>,
    instrument_id: InstrumentId,
    order_side: OrderSide,
    before_emit: B,
    mut emit: F,
) -> anyhow::Result<Vec<BufferedFillEmission>>
where
    B: FnOnce(&[BufferedFill]),
    F: FnMut(&BufferedFill, Option<Quantity>) -> OrderFilled,
{
    let fills = prepare_pending_fills(
        inner,
        venue_order_id,
        client_order_id,
        instrument_id,
        order_side,
    )?;
    let mut prospective = inner.orders.get(&venue_order_id).cloned().ok_or_else(|| {
        anyhow::anyhow!("cannot drain fills for unregistered order {venue_order_id}")
    })?;
    let mut admissions = Vec::with_capacity(fills.len());
    for buffered in &fills {
        let is_voided = buffered
            .correction
            .as_ref()
            .is_some_and(|correction| inner.voided_trades.contains(&correction.correction_key));
        admissions.push(if is_voided {
            None
        } else {
            Some(admit_fill_in(
                &mut prospective,
                &buffered.report,
                &buffered.report.venue_order_id,
            )?)
        });
    }

    inner.pending_fills.remove(&venue_order_id);
    inner.orders.insert(venue_order_id, prospective);
    before_emit(&fills);
    let mut emissions = Vec::with_capacity(fills.len());
    for (buffered, admission) in fills.into_iter().zip(admissions) {
        if let Some(correction) = buffered.correction.as_ref()
            && inner.voided_trades.contains(&correction.correction_key)
        {
            emissions.push(BufferedFillEmission {
                buffered,
                emitted: false,
            });
            continue;
        }

        let Some(ProspectiveFillAdmission::New { quantity_update }) = admission else {
            emissions.push(BufferedFillEmission {
                buffered,
                emitted: false,
            });
            continue;
        };

        let filled = emit(&buffered, quantity_update);
        if let Some(correction) = buffered.correction.as_ref() {
            if let Some(applied) = inner
                .applied_buffered_fills
                .get_mut(&correction.correction_key)
            {
                applied.fills.push(filled);
                applied.is_confirmed |= correction.is_confirmed;
            } else {
                inner.applied_buffered_fills.insert(
                    correction.correction_key.clone(),
                    AppliedBufferedCorrection {
                        fills: vec![filled],
                        is_confirmed: correction.is_confirmed,
                    },
                );
            }
        }
        emissions.push(BufferedFillEmission {
            buffered,
            emitted: true,
        });
    }
    Ok(emissions)
}

fn promote_pending_correction(
    fills: Option<&mut Vec<BufferedFill>>,
    correction_key: &str,
    raw_trade_id: &str,
    raw_corrective_timestamp: &str,
) {
    let Some(fills) = fills else {
        return;
    };

    for fill in fills {
        let Some(correction) = fill.correction.as_mut() else {
            continue;
        };

        if correction.correction_key == correction_key {
            correction.is_confirmed = true;
            correction.raw_trade_id = raw_trade_id.to_string();
            correction.raw_corrective_timestamp = raw_corrective_timestamp.to_string();
        }
    }
}

fn push_buffered<V>(
    buffer: &mut FifoCacheMap<VenueOrderId, Vec<V>, 1_000>,
    venue_order_id: VenueOrderId,
    value: V,
) {
    if let Some(values) = buffer.get_mut(&venue_order_id) {
        values.push(value);
    } else {
        buffer.insert(venue_order_id, vec![value]);
    }
}

#[cfg(test)]
impl OrderFillTrackerMap {
    /// Registers an order directly, for tests that set up an already-accepted order.
    pub(crate) fn register(
        &self,
        venue_order_id: VenueOrderId,
        submitted_qty: Quantity,
        _order_side: OrderSide,
        _instrument_id: InstrumentId,
        _size_precision: u8,
        _price_precision: u8,
    ) {
        self.inner.lock().expect(MUTEX_POISONED).orders.insert(
            venue_order_id,
            new_order_state(submitted_qty, FillGrowthPolicy::Fixed),
        );
    }

    /// Records a fill against a registered order, for tests that drive fill accumulation directly.
    pub(crate) fn record_fill(&self, venue_order_id: &VenueOrderId, qty: Quantity) {
        record_fill_in(
            &mut self.inner.lock().expect(MUTEX_POISONED).orders,
            venue_order_id,
            qty,
        );
    }

    /// Buffers a fill as if it arrived on the WS channel before the order was registered.
    pub(crate) fn buffer_fill_for_test(&self, venue_order_id: VenueOrderId, report: FillReport) {
        push_buffered(
            &mut self.inner.lock().expect(MUTEX_POISONED).pending_fills,
            venue_order_id,
            BufferedFill {
                report,
                correction: None,
            },
        );
    }

    /// Buffers an order report as if it arrived on the WS channel before the order was registered.
    pub(crate) fn buffer_report_for_test(
        &self,
        venue_order_id: VenueOrderId,
        report: OrderStatusReport,
    ) {
        self.buffer_report(venue_order_id, report);
    }

    /// Returns true if a fill is currently buffered for the order.
    pub(crate) fn has_pending_fill(&self, venue_order_id: &VenueOrderId) -> bool {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .pending_fills
            .contains_key(venue_order_id)
    }

    /// Returns the fills currently buffered for the order.
    pub(crate) fn pending_fills_for(&self, venue_order_id: &VenueOrderId) -> Vec<FillReport> {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .pending_fills
            .get(venue_order_id)
            .map(|fills| fills.iter().map(|fill| fill.report.clone()).collect())
            .unwrap_or_default()
    }

    /// Returns buffered fill records, including correction metadata, for a test assertion.
    pub(crate) fn pending_buffered_fills_for(
        &self,
        venue_order_id: &VenueOrderId,
    ) -> Vec<BufferedFill> {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .pending_fills
            .get(venue_order_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Returns true if an order report is currently buffered for the order.
    pub(crate) fn has_pending_report(&self, venue_order_id: &VenueOrderId) -> bool {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .pending_reports
            .contains_key(venue_order_id)
    }
}

#[cfg(test)]
fn record_fill_in(
    orders: &mut AHashMap<VenueOrderId, OrderFillState>,
    venue_order_id: &VenueOrderId,
    qty: Quantity,
) {
    if let Some(s) = orders.get_mut(venue_order_id) {
        s.cumulative_filled = s.cumulative_filled + qty;
    }
}

fn reverse_fill_in(
    orders: &mut AHashMap<VenueOrderId, OrderFillState>,
    venue_order_id: &VenueOrderId,
    trade_id: TradeId,
    last_px: Price,
    qty: Quantity,
) {
    if let Some(state) = orders.get_mut(venue_order_id) {
        state.applied_fills.remove(&trade_id);
        if matches!(
            state.growth_policy,
            FillGrowthPolicy::QuoteImmediateBuy { .. }
        ) {
            let remaining = qty
                .as_decimal()
                .checked_mul(last_px.as_decimal())
                .and_then(|fill_notional| {
                    state.cumulative_quote_notional.checked_sub(fill_notional)
                })
                .filter(|remaining| *remaining >= Decimal::ZERO);
            if let Some(remaining) = remaining {
                state.cumulative_quote_notional = remaining;
            }
        }
        state.cumulative_filled = if qty >= state.cumulative_filled {
            Quantity::zero(state.cumulative_filled.precision)
        } else {
            state.cumulative_filled - qty
        };
    }
}

fn snap_fill_qty_in(
    orders: &AHashMap<VenueOrderId, OrderFillState>,
    venue_order_id: &VenueOrderId,
    fill_qty: Quantity,
) -> Quantity {
    match orders.get(venue_order_id) {
        Some(OrderFillState {
            growth_policy:
                FillGrowthPolicy::QuoteImmediateBuy { .. } | FillGrowthPolicy::QuoteImmediateBuyUnproven,
            ..
        }) => fill_qty,
        Some(s) => {
            let diff = s.submitted_qty.as_decimal() - fill_qty.as_decimal();
            if diff < Decimal::ZERO && diff.abs() < DUST_SNAP_THRESHOLD_DEC {
                log::debug!(
                    "Snapping overfill {fill_qty} -> {} (dust={diff})",
                    s.submitted_qty,
                );
                s.submitted_qty
            } else {
                fill_qty
            }
        }
        None => fill_qty,
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{LiquiditySide, OrderStatus, OrderType, TimeInForce},
        identifiers::{AccountId, TradeId},
        types::{Currency, Money, Price},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    fn pusd() -> Currency {
        Currency::pUSD()
    }

    fn test_fill_report(
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        trade_id: &str,
        last_qty: Quantity,
    ) -> FillReport {
        FillReport {
            account_id: AccountId::from("POLY-001"),
            instrument_id,
            venue_order_id,
            trade_id: TradeId::from(trade_id),
            order_side: OrderSide::Buy,
            last_qty,
            last_px: Price::new(0.55, 2),
            commission: Money::zero(pusd()),
            liquidity_side: LiquiditySide::Taker,
            avg_px: None,
            report_id: UUID4::new(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            client_order_id: None,
            venue_position_id: None,
        }
    }

    fn test_order_report(
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        order_status: OrderStatus,
    ) -> OrderStatusReport {
        OrderStatusReport::new(
            AccountId::from("POLY-001"),
            instrument_id,
            Some(ClientOrderId::from("O-REPORT-RACE")),
            venue_order_id,
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            order_status,
            Quantity::new(100.0, 6),
            Quantity::zero(6),
            UnixNanos::from(1_000u64),
            UnixNanos::from(1_000u64),
            UnixNanos::from(1_000u64),
            None,
        )
    }

    fn test_order_filled(report: &FillReport, client_order_id: ClientOrderId) -> OrderFilled {
        use nautilus_model::{
            enums::OrderType,
            identifiers::{StrategyId, TraderId},
        };

        OrderFilled::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            report.instrument_id,
            client_order_id,
            report.venue_order_id,
            report.account_id,
            report.trade_id,
            report.order_side,
            OrderType::Limit,
            report.last_qty,
            report.last_px,
            pusd(),
            report.liquidity_side,
            UUID4::new(),
            report.ts_event,
            report.ts_init,
            false,
            None,
            Some(report.commission),
            None,
        )
    }

    #[rstest]
    fn test_register_and_contains() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        assert!(!tracker.contains(&vid));

        tracker.register(
            vid,
            Quantity::from("100"),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );
        assert!(tracker.contains(&vid));
    }

    #[rstest]
    fn test_register_retains_fill_state_after_later_capacity_flood() {
        let tracker = OrderFillTrackerMap::new();
        let retained = VenueOrderId::from("order-retain");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        tracker.register(
            retained,
            Quantity::from("100"),
            OrderSide::Buy,
            instrument_id,
            6,
            2,
        );

        for index in 0..10_000 {
            tracker.register(
                VenueOrderId::from(format!("order-flood-{index}").as_str()),
                Quantity::from("1"),
                OrderSide::Sell,
                instrument_id,
                6,
                2,
            );
        }

        assert!(tracker.contains(&retained));
        assert_eq!(
            tracker.submitted_qty(&retained),
            Some(Quantity::from("100"))
        );
    }

    #[rstest]
    fn test_failed_trade_suppresses_buffered_fill_drained_later() {
        use std::cell::Cell;

        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("order-failed-before-drain");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let report = test_fill_report(
            instrument_id,
            venue_order_id,
            "trade-failed-before-drain",
            Quantity::new(5.0, 6),
        );
        let correction_key = "trade-failed-before-drain-order-failed-before-drain";

        let accepted = tracker.accept_or_buffer_fill(
            venue_order_id,
            report.clone(),
            FillCorrectionMetadata {
                correction_key: correction_key.to_string(),
                raw_trade_id: "trade-failed-before-drain".to_string(),
                raw_corrective_timestamp: "1700000000000".to_string(),
                info: None,
                is_confirmed: false,
            },
            |_| Ok(false),
        );
        let prior_fills = tracker.void_buffered_trade(correction_key).unwrap();
        tracker.register_without_draining(
            venue_order_id,
            Quantity::new(10.0, 6),
            FillGrowthPolicy::Fixed,
        );
        let fill = test_order_filled(&report, ClientOrderId::from("O-FAILED-BEFORE-DRAIN"));
        let was_emitted = Cell::new(false);
        let emissions = tracker
            .emit_pending_fills_for_registered(
                venue_order_id,
                Some(ClientOrderId::from("O-FAILED-BEFORE-DRAIN")),
                instrument_id,
                OrderSide::Buy,
                |_| {},
                |_, _| {
                    was_emitted.set(true);
                    fill.clone()
                },
            )
            .unwrap();

        assert!(accepted.unwrap().is_none());
        assert!(prior_fills.is_empty());
        assert_eq!(emissions.len(), 1);
        assert!(!emissions[0].emitted);
        assert!(!was_emitted.get());
        assert_eq!(
            tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::zero(6))
        );
    }

    #[rstest]
    fn test_tombstoned_fill_does_not_expand_live_fill_quantity() {
        use std::cell::{Cell, RefCell};

        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("order-mixed-tombstone");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let live_report = test_fill_report(
            instrument_id,
            venue_order_id,
            "trade-live",
            Quantity::new(60.0, 6),
        );
        let voided_report = test_fill_report(
            instrument_id,
            venue_order_id,
            "trade-voided",
            Quantity::new(60.0, 6),
        );

        for (report, correction_key) in [
            (live_report, "trade-live-order"),
            (voided_report, "trade-voided-order"),
        ] {
            assert!(
                tracker
                    .accept_or_buffer_fill(
                        venue_order_id,
                        report,
                        FillCorrectionMetadata {
                            correction_key: correction_key.to_string(),
                            raw_trade_id: correction_key.to_string(),
                            raw_corrective_timestamp: "1700000000000".to_string(),
                            info: None,
                            is_confirmed: false,
                        },
                        |_| Ok(false),
                    )
                    .unwrap()
                    .is_none()
            );
        }
        assert!(
            tracker
                .void_buffered_trade("trade-voided-order")
                .unwrap()
                .is_empty()
        );
        tracker.register_without_draining(
            venue_order_id,
            Quantity::new(100.0, 6),
            FillGrowthPolicy::Fixed,
        );

        let emitted_count = Cell::new(0);
        let quantity_bumps = RefCell::new(Vec::new());
        let client_order_id = ClientOrderId::from("O-MIXED-TOMBSTONE");
        let emissions = tracker
            .emit_pending_fills_for_registered(
                venue_order_id,
                Some(client_order_id),
                instrument_id,
                OrderSide::Buy,
                |_| {},
                |buffered, new_qty| {
                    emitted_count.set(emitted_count.get() + 1);
                    quantity_bumps.borrow_mut().push(new_qty);
                    test_order_filled(&buffered.report, client_order_id)
                },
            )
            .unwrap();

        assert_eq!(emissions.len(), 2);
        assert_eq!(
            emissions.iter().filter(|emission| emission.emitted).count(),
            1
        );
        assert_eq!(emitted_count.get(), 1);
        assert_eq!(&*quantity_bumps.borrow(), &[None]);
        assert_eq!(
            tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::new(60.0, 6))
        );
        assert_eq!(
            tracker.submitted_qty(&venue_order_id),
            Some(Quantity::new(100.0, 6))
        );
    }

    #[rstest]
    fn test_confirmation_during_drain_observes_applied_authority() {
        use std::{
            sync::{Arc, mpsc},
            thread,
        };

        let tracker = Arc::new(OrderFillTrackerMap::new());
        let venue_order_id = VenueOrderId::from("order-confirm-during-drain");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let correction_key = "trade-confirm-during-drain-order";
        let report = test_fill_report(
            instrument_id,
            venue_order_id,
            "trade-confirm-during-drain",
            Quantity::new(10.0, 6),
        );
        assert!(
            tracker
                .accept_or_buffer_fill(
                    venue_order_id,
                    report.clone(),
                    FillCorrectionMetadata {
                        correction_key: correction_key.to_string(),
                        raw_trade_id: "trade-confirm-during-drain".to_string(),
                        raw_corrective_timestamp: "1700000000000".to_string(),
                        info: None,
                        is_confirmed: false,
                    },
                    |_| Ok(false),
                )
                .unwrap()
                .is_none()
        );
        tracker.register_without_draining(
            venue_order_id,
            Quantity::new(100.0, 6),
            FillGrowthPolicy::Fixed,
        );

        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let drain_tracker = Arc::clone(&tracker);

        let drain = thread::spawn(move || {
            let filled = test_order_filled(&report, ClientOrderId::from("O-CONFIRM-DURING-DRAIN"));
            drain_tracker
                .emit_pending_fills_for_registered(
                    venue_order_id,
                    Some(ClientOrderId::from("O-CONFIRM-DURING-DRAIN")),
                    instrument_id,
                    OrderSide::Buy,
                    |_| {
                        entered_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                    },
                    |_, _| filled.clone(),
                )
                .unwrap()
        });
        entered_rx.recv().unwrap();

        let confirm_tracker = Arc::clone(&tracker);
        let confirm = thread::spawn(move || {
            confirm_tracker.promote_pending_trade_confirmed(
                &[venue_order_id],
                correction_key,
                "trade-confirm-during-drain",
                "1700000000123",
            )
        });
        release_tx.send(()).unwrap();

        let emissions = drain.join().unwrap();
        assert_eq!(emissions.len(), 1);
        assert!(emissions[0].emitted);
        assert!(confirm.join().unwrap());
        let evidence = tracker.correction_fill_evidence(correction_key);
        assert!(evidence.pending.is_empty());
        assert_eq!(evidence.applied.len(), 1);
    }

    #[rstest]
    fn test_identity_registration_race_cannot_strand_fill_after_drain() {
        use std::{
            sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
                mpsc,
            },
            thread,
        };

        let tracker = Arc::new(OrderFillTrackerMap::new());
        let identity_available = Arc::new(AtomicBool::new(false));
        let venue_order_id = VenueOrderId::from("order-identity-race");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let report = test_fill_report(
            instrument_id,
            venue_order_id,
            "trade-identity-race",
            Quantity::new(10.0, 6),
        );
        let (decision_tx, decision_rx) = mpsc::channel();
        let (identity_tx, identity_rx) = mpsc::channel();

        let ws_tracker = Arc::clone(&tracker);
        let ws_identity = Arc::clone(&identity_available);

        let ws = thread::spawn(move || {
            ws_tracker.accept_or_buffer_fill(
                venue_order_id,
                report,
                FillCorrectionMetadata {
                    correction_key: "trade-identity-race-order".to_string(),
                    raw_trade_id: "trade-identity-race".to_string(),
                    raw_corrective_timestamp: "1700000000000".to_string(),
                    info: None,
                    is_confirmed: false,
                },
                |_| {
                    decision_tx.send(()).unwrap();
                    identity_rx.recv().unwrap();
                    Ok(ws_identity.load(Ordering::Acquire))
                },
            )
        });
        decision_rx.recv().unwrap();
        identity_available.store(true, Ordering::Release);

        let response_tracker = Arc::clone(&tracker);
        let response = thread::spawn(move || {
            response_tracker.register_without_draining(
                venue_order_id,
                Quantity::new(100.0, 6),
                FillGrowthPolicy::Fixed,
            );
            response_tracker
                .emit_pending_fills_for_registered(
                    venue_order_id,
                    Some(ClientOrderId::from("O-IDENTITY-RACE")),
                    instrument_id,
                    OrderSide::Buy,
                    |_| {},
                    |buffered, _| {
                        test_order_filled(&buffered.report, ClientOrderId::from("O-IDENTITY-RACE"))
                    },
                )
                .unwrap()
        });
        identity_tx.send(()).unwrap();

        assert!(ws.join().unwrap().unwrap().is_none());
        let emissions = response.join().unwrap();
        assert_eq!(emissions.len(), 1);
        assert!(emissions[0].emitted);
        assert!(!tracker.has_pending_fill(&venue_order_id));
        assert_eq!(
            tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::new(10.0, 6))
        );
    }

    #[rstest]
    fn test_binding_callback_error_retains_fill_without_authority() {
        use std::cell::Cell;

        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("order-binding-callback-error");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let report = test_fill_report(
            instrument_id,
            venue_order_id,
            "trade-binding-callback-error",
            Quantity::new(10.0, 6),
        );
        tracker.register_without_draining(
            venue_order_id,
            Quantity::new(100.0, 6),
            FillGrowthPolicy::Fixed,
        );

        let result = tracker.accept_or_buffer_fill(
            venue_order_id,
            report,
            FillCorrectionMetadata {
                correction_key: "trade-binding-callback-error-order".to_string(),
                raw_trade_id: "trade-binding-callback-error".to_string(),
                raw_corrective_timestamp: "1700000000000".to_string(),
                info: None,
                is_confirmed: false,
            },
            |_| anyhow::bail!("identity changed before the atomic decision"),
        );

        assert!(result.is_err());
        assert!(tracker.has_pending_fill(&venue_order_id));
        assert_eq!(
            tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::zero(6))
        );

        let terminal_emitted = Cell::new(false);
        let rejected = OrderStatusReport::new(
            AccountId::from("POLY-001"),
            instrument_id,
            Some(ClientOrderId::from("O-BINDING-CALLBACK-ERROR")),
            venue_order_id,
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Rejected,
            Quantity::new(100.0, 6),
            Quantity::zero(6),
            UnixNanos::from(1_000u64),
            UnixNanos::from(1_000u64),
            UnixNanos::from(1_000u64),
            None,
        );
        assert!(!tracker.emit_or_buffer_report_if_no_pending_fill(
            venue_order_id,
            rejected,
            |_| terminal_emitted.set(true),
        ));
        assert!(!terminal_emitted.get());
        assert!(tracker.has_pending_report(&venue_order_id));
    }

    #[rstest]
    fn test_late_bound_report_cannot_be_buffered_behind_atomic_drain() {
        use std::{
            sync::{Arc, Barrier},
            thread,
        };

        let tracker = Arc::new(OrderFillTrackerMap::new());
        let venue_order_id = VenueOrderId::from("order-report-drain-race");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        tracker.buffer_report_for_test(
            venue_order_id,
            test_order_report(instrument_id, venue_order_id, OrderStatus::Rejected),
        );

        let classifier_entered = Arc::new(Barrier::new(2));
        let release_classifier = Arc::new(Barrier::new(2));
        let drain_tracker = Arc::clone(&tracker);
        let drain_entered = Arc::clone(&classifier_entered);
        let drain_release = Arc::clone(&release_classifier);

        let drain = thread::spawn(move || {
            drain_tracker.classify_pending_order_reports(
                venue_order_id,
                Quantity::new(100.0, 6),
                FillGrowthPolicy::Fixed,
                |_| {
                    drain_entered.wait();
                    drain_release.wait();
                    true
                },
            )
        });

        classifier_entered.wait();
        let ingress_started = Arc::new(Barrier::new(2));
        let ingress_tracker = Arc::clone(&tracker);
        let ingress_barrier = Arc::clone(&ingress_started);
        let ingress = thread::spawn(move || {
            ingress_barrier.wait();
            ingress_tracker.accept_or_buffer_report(
                venue_order_id,
                test_order_report(instrument_id, venue_order_id, OrderStatus::Canceled),
                || true,
            )
        });
        ingress_started.wait();
        release_classifier.wait();

        let drained = drain.join().expect("drain thread panicked");
        let late = ingress.join().expect("ingress thread panicked");
        assert!(matches!(
            drained,
            PendingOrderReportDrain::Rejected(ref reports)
                if reports.len() == 1 && reports[0].order_status == OrderStatus::Rejected
        ));
        assert!(matches!(
            late,
            Some((ref report, false)) if report.order_status == OrderStatus::Canceled
        ));
        assert!(!tracker.has_pending_report(&venue_order_id));
    }

    #[rstest]
    fn test_binding_error_replay_drains_exactly_one_retained_fill() {
        use std::cell::Cell;

        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("order-binding-error-replay");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let report = test_fill_report(
            instrument_id,
            venue_order_id,
            "trade-binding-error-replay",
            Quantity::new(10.0, 6),
        );
        let correction = FillCorrectionMetadata {
            correction_key: "trade-binding-error-replay-order".to_string(),
            raw_trade_id: "trade-binding-error-replay".to_string(),
            raw_corrective_timestamp: "1700000000000".to_string(),
            info: None,
            is_confirmed: false,
        };

        let first = tracker
            .accept_or_buffer_fills(
                vec![(venue_order_id, report.clone(), correction.clone())],
                |_| -> anyhow::Result<Option<()>> {
                    anyhow::bail!("identity changed before the atomic decision")
                },
            )
            .unwrap();
        assert!(first.binding_error.is_some());
        assert!(first.reports.iter().all(Option::is_none));
        assert_eq!(tracker.pending_fills_for(&venue_order_id).len(), 1);

        tracker.register_without_draining(
            venue_order_id,
            Quantity::new(100.0, 6),
            FillGrowthPolicy::Fixed,
        );
        let replay = tracker
            .accept_or_buffer_fills(vec![(venue_order_id, report, correction)], |_| Ok(Some(())))
            .unwrap();
        assert!(replay.binding_error.is_none());
        assert!(replay.reports.iter().all(Option::is_none));
        assert_eq!(tracker.pending_fills_for(&venue_order_id).len(), 1);
        assert_eq!(
            tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::zero(6))
        );

        let emitted = Cell::new(0);
        let emissions = tracker
            .emit_pending_fills_for_registered(
                venue_order_id,
                Some(ClientOrderId::from("O-BINDING-ERROR-REPLAY")),
                instrument_id,
                OrderSide::Buy,
                |_| {},
                |buffered, _| {
                    emitted.set(emitted.get() + 1);
                    test_order_filled(
                        &buffered.report,
                        ClientOrderId::from("O-BINDING-ERROR-REPLAY"),
                    )
                },
            )
            .unwrap();

        assert_eq!(emitted.get(), 1);
        assert_eq!(
            emissions.iter().filter(|emission| emission.emitted).count(),
            1
        );
        assert_eq!(
            tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::new(10.0, 6))
        );
    }

    #[rstest]
    fn test_binding_error_buffers_entire_maker_batch_before_authority() {
        let tracker = OrderFillTrackerMap::new();
        let first_order = VenueOrderId::from("maker-order-batch-first");
        let second_order = VenueOrderId::from("maker-order-batch-second");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");

        for venue_order_id in [first_order, second_order] {
            tracker.register_without_draining(
                venue_order_id,
                Quantity::new(100.0, 6),
                FillGrowthPolicy::Fixed,
            );
        }
        let first_report = test_fill_report(
            instrument_id,
            first_order,
            "maker-trade-batch-first",
            Quantity::new(10.0, 6),
        );
        let second_report = test_fill_report(
            instrument_id,
            second_order,
            "maker-trade-batch-second",
            Quantity::new(20.0, 6),
        );
        let metadata = |raw_trade_id: &str| FillCorrectionMetadata {
            correction_key: "maker-trade-batch-correction".to_string(),
            raw_trade_id: raw_trade_id.to_string(),
            raw_corrective_timestamp: "1700000000000".to_string(),
            info: None,
            is_confirmed: false,
        };

        let outcome = tracker
            .accept_or_buffer_fills(
                vec![
                    (
                        first_order,
                        first_report,
                        metadata("maker-trade-batch-first"),
                    ),
                    (
                        second_order,
                        second_report,
                        metadata("maker-trade-batch-second"),
                    ),
                ],
                |report| {
                    if report.venue_order_id == second_order {
                        anyhow::bail!("second maker identity changed before admission");
                    }
                    Ok(Some(()))
                },
            )
            .unwrap();

        assert!(outcome.binding_error.is_some());
        assert!(outcome.reports.iter().all(Option::is_none));
        assert_eq!(tracker.pending_fills_for(&first_order).len(), 1);
        assert_eq!(tracker.pending_fills_for(&second_order).len(), 1);
        assert_eq!(
            tracker.get_cumulative_filled(&first_order),
            Some(Quantity::zero(6))
        );
        assert_eq!(
            tracker.get_cumulative_filled(&second_order),
            Some(Quantity::zero(6))
        );
    }

    #[rstest]
    fn test_buffered_fill_batch_overfill_retains_every_fill_and_mutates_nothing() {
        use std::cell::Cell;

        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("order-buffered-aggregate-overfill");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let correction = |raw_trade_id: &str| FillCorrectionMetadata {
            correction_key: "buffered-aggregate-overfill".to_string(),
            raw_trade_id: raw_trade_id.to_string(),
            raw_corrective_timestamp: "1700000000000".to_string(),
            info: None,
            is_confirmed: false,
        };
        let fills = ["trade-buffered-first", "trade-buffered-second"]
            .into_iter()
            .map(|raw_trade_id| {
                (
                    venue_order_id,
                    test_fill_report(
                        instrument_id,
                        venue_order_id,
                        raw_trade_id,
                        Quantity::new(6.0, 6),
                    ),
                    correction(raw_trade_id),
                )
            })
            .collect();

        let admission = tracker
            .accept_or_buffer_fills(fills, |_| Ok(None::<()>))
            .unwrap();
        assert!(admission.reports.iter().all(Option::is_none));
        tracker.register_without_draining(
            venue_order_id,
            Quantity::new(10.0, 6),
            FillGrowthPolicy::Fixed,
        );

        let emitted = Cell::new(0);
        let result = tracker.emit_pending_fills_for_registered(
            venue_order_id,
            Some(ClientOrderId::from("O-BUFFERED-AGGREGATE-OVERFILL")),
            instrument_id,
            OrderSide::Buy,
            |_| {},
            |buffered, _| {
                emitted.set(emitted.get() + 1);
                test_order_filled(
                    &buffered.report,
                    ClientOrderId::from("O-BUFFERED-AGGREGATE-OVERFILL"),
                )
            },
        );

        assert!(result.is_err());
        assert_eq!(emitted.get(), 0);
        assert_eq!(tracker.pending_fills_for(&venue_order_id).len(), 2);
        assert_eq!(
            tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::zero(6))
        );
    }

    #[rstest]
    fn test_quote_growth_rejects_fill_above_signed_budget_without_mutation() {
        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("order-quote-budget-overfill");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        tracker.register_without_draining(
            venue_order_id,
            Quantity::new(10.0, 6),
            FillGrowthPolicy::quote_immediate_buy(dec!(6.00)),
        );
        let report = test_fill_report(
            instrument_id,
            venue_order_id,
            "trade-quote-budget-overfill",
            Quantity::new(11.0, 6),
        );

        let error = tracker
            .accept_or_buffer_fills(
                vec![(
                    venue_order_id,
                    report,
                    FillCorrectionMetadata {
                        correction_key: "quote-budget-overfill".to_string(),
                        raw_trade_id: "trade-quote-budget-overfill".to_string(),
                        raw_corrective_timestamp: "1700000000000".to_string(),
                        info: None,
                        is_confirmed: false,
                    },
                )],
                |_| Ok(Some(())),
            )
            .expect_err("fill notional above the signed quote budget must fail closed");

        assert!(error.to_string().contains("signed quote budget"));
        assert_eq!(
            tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::zero(6))
        );
        assert_eq!(
            tracker.submitted_qty(&venue_order_id),
            Some(Quantity::new(10.0, 6))
        );
    }

    #[rstest]
    fn test_quote_growth_enforces_signed_budget_across_fills() {
        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("order-quote-budget-cumulative");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        tracker.register_without_draining(
            venue_order_id,
            Quantity::new(10.0, 6),
            FillGrowthPolicy::quote_immediate_buy(dec!(6.00)),
        );
        let metadata = |trade_id: &str| FillCorrectionMetadata {
            correction_key: trade_id.to_string(),
            raw_trade_id: trade_id.to_string(),
            raw_corrective_timestamp: "1700000000000".to_string(),
            info: None,
            is_confirmed: false,
        };

        let first = test_fill_report(
            instrument_id,
            venue_order_id,
            "trade-quote-budget-first",
            Quantity::new(5.0, 6),
        );
        tracker
            .accept_or_buffer_fills(
                vec![(venue_order_id, first, metadata("trade-quote-budget-first"))],
                |_| Ok(Some(())),
            )
            .unwrap();

        let second = test_fill_report(
            instrument_id,
            venue_order_id,
            "trade-quote-budget-second",
            Quantity::new(7.0, 6),
        );
        let error = tracker
            .accept_or_buffer_fills(
                vec![(
                    venue_order_id,
                    second,
                    metadata("trade-quote-budget-second"),
                )],
                |_| Ok(Some(())),
            )
            .expect_err("cumulative quote notional above the signed budget must fail closed");

        assert!(error.to_string().contains("signed quote budget"));
        assert_eq!(
            tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::new(5.0, 6))
        );
    }

    #[rstest]
    fn test_confirmed_fill_validation_unions_cached_and_tracker_authority() {
        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("order-cache-tracker-union");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        tracker.register_without_draining(
            venue_order_id,
            Quantity::new(10.0, 6),
            FillGrowthPolicy::Fixed,
        );

        let tracked_report = test_fill_report(
            instrument_id,
            venue_order_id,
            "trade-tracker-union",
            Quantity::new(4.0, 6),
        );
        tracker
            .accept_or_buffer_fills(
                vec![(
                    venue_order_id,
                    tracked_report,
                    FillCorrectionMetadata {
                        correction_key: "tracker-union".to_string(),
                        raw_trade_id: "trade-tracker-union".to_string(),
                        raw_corrective_timestamp: "1700000000000".to_string(),
                        info: None,
                        is_confirmed: true,
                    },
                )],
                |_| Ok(Some(())),
            )
            .unwrap();

        let cached_report = test_fill_report(
            instrument_id,
            venue_order_id,
            "trade-cache-union",
            Quantity::new(4.0, 6),
        );
        let cached_fill =
            test_order_filled(&cached_report, ClientOrderId::from("O-CACHE-TRACKER-UNION"));
        let returned_report = test_fill_report(
            instrument_id,
            venue_order_id,
            "trade-returned-union",
            Quantity::new(4.0, 6),
        );

        let error = tracker
            .validate_confirmed_fills(&[cached_fill], &[returned_report])
            .expect_err("cache, tracker, and returned fills must form one aggregate");

        assert!(error.to_string().contains("exceeds submitted quantity"));
        assert_eq!(
            tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::new(4.0, 6))
        );
    }

    #[rstest]
    fn test_partial_batch_replay_does_not_buffer_already_emitted_sibling() {
        use std::cell::Cell;

        let tracker = OrderFillTrackerMap::new();
        let first_order = VenueOrderId::from("maker-order-replay-applied");
        let second_order = VenueOrderId::from("maker-order-replay-pending");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        tracker.register_without_draining(
            first_order,
            Quantity::new(100.0, 6),
            FillGrowthPolicy::Fixed,
        );
        let first_report = test_fill_report(
            instrument_id,
            first_order,
            "maker-trade-replay-applied",
            Quantity::new(10.0, 6),
        );
        let second_report = test_fill_report(
            instrument_id,
            second_order,
            "maker-trade-replay-pending",
            Quantity::new(20.0, 6),
        );
        let metadata = |raw_trade_id: &str| FillCorrectionMetadata {
            correction_key: "maker-partial-batch-replay".to_string(),
            raw_trade_id: raw_trade_id.to_string(),
            raw_corrective_timestamp: "1700000000000".to_string(),
            info: None,
            is_confirmed: false,
        };
        let batch = || {
            vec![
                (
                    first_order,
                    first_report.clone(),
                    metadata("maker-trade-replay-applied"),
                ),
                (
                    second_order,
                    second_report.clone(),
                    metadata("maker-trade-replay-pending"),
                ),
            ]
        };

        let first = tracker
            .accept_or_buffer_fills(batch(), |_| Ok(Some(())))
            .unwrap();
        assert!(first.binding_error.is_none());
        assert!(first.reports[0].is_some());
        assert!(first.reports[1].is_none());
        assert!(!tracker.has_pending_fill(&first_order));
        assert!(tracker.has_pending_fill(&second_order));
        assert_eq!(
            tracker.get_cumulative_filled(&first_order),
            Some(Quantity::new(10.0, 6))
        );

        let replay = tracker
            .accept_or_buffer_fills(batch(), |_| Ok(Some(())))
            .unwrap();
        assert!(replay.binding_error.is_none());
        assert!(replay.reports.iter().all(Option::is_none));
        assert!(!tracker.has_pending_fill(&first_order));
        assert_eq!(tracker.pending_fills_for(&second_order).len(), 1);
        assert_eq!(
            tracker.get_cumulative_filled(&first_order),
            Some(Quantity::new(10.0, 6))
        );

        let duplicate_emissions = Cell::new(0);
        let drained = tracker
            .emit_pending_fills_for_registered(
                first_order,
                Some(ClientOrderId::from("O-PARTIAL-BATCH-REPLAY")),
                instrument_id,
                OrderSide::Buy,
                |_| {},
                |buffered, _| {
                    duplicate_emissions.set(duplicate_emissions.get() + 1);
                    test_order_filled(
                        &buffered.report,
                        ClientOrderId::from("O-PARTIAL-BATCH-REPLAY"),
                    )
                },
            )
            .unwrap();
        assert!(drained.is_empty());
        assert_eq!(duplicate_emissions.get(), 0);
        assert_eq!(
            tracker.get_cumulative_filled(&first_order),
            Some(Quantity::new(10.0, 6))
        );
    }

    // snap_fill_qty is overfill-only. Underfill is preserved so partial fills
    // followed by cancel keep their venue-reported size; terminal quantity
    // normalization handles CLOB cent-tick truncation without a synthetic fill.
    #[rstest]
    // Underfill within the dust band: NOT snapped. The fill is recorded
    // as-is; terminal normalization later lowers the order quantity.
    #[case::underfill_dust_preserved(23.696681, 23.690000, 23.690000)]
    #[case::underfill_near_band_preserved(100.000000, 99.990100, 99.990100)]
    // Underfill at exactly the band: NOT snapped.
    #[case::underfill_at_band(100.000000, 99.990000, 99.990000)]
    // Underfill above the band: NOT snapped (real partial leaves).
    #[case::underfill_above_band(100.000000, 99.980000, 99.980000)]
    // Underfill far past band: NOT snapped.
    #[case::large_underfill(100.000000, 50.000000, 50.000000)]
    // Overfill within the band: V2 market BUY where the SDK truncates the
    // registered base qty to USDC scale but the on-chain fill comes back at
    // full precision. Observed production drift is 4-66 ulps. Snap DOWN so
    // the engine does not reject as overfill.
    #[case::overfill_dust(714.285710, 714.285714, 714.285710)]
    // Overfill near the band (0.0099 < 0.01): still snaps.
    #[case::overfill_near_band(100.000000, 100.009900, 100.000000)]
    // Overfill at exactly the band must NOT snap (exclusive boundary).
    #[case::overfill_at_band(100.000000, 100.010000, 100.010000)]
    // Overfill above the band: leave fill alone, surfaces as engine-side
    // error since this is no longer dust.
    #[case::overfill_above_band(100.000000, 100.020000, 100.020000)]
    // Overfill far past band: leave fill alone.
    #[case::large_overfill(100.000000, 150.000000, 150.000000)]
    // Exact match: no-op (returns the fill qty, which equals submitted).
    #[case::exact(100.000000, 100.000000, 100.000000)]
    fn test_snap_fill_qty(#[case] submitted: f64, #[case] fill: f64, #[case] expected: f64) {
        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("order-1");
        tracker.register(
            venue_order_id,
            Quantity::new(submitted, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        let snapped = tracker.snap_fill_qty(&venue_order_id, Quantity::new(fill, 6));
        assert_eq!(snapped, Quantity::new(expected, 6));
    }

    // The band is in absolute share units; it does not scale with
    // size_precision. CLOB cent-tick truncation and V2 USDC-scale truncation
    // are both fixed in absolute share terms, so the threshold is too.
    // snap_fill_qty is overfill-only, so underfill cases pass through.
    #[rstest]
    #[case::underfill_within_band_preserved(100.000, 99.995, 99.995)]
    #[case::underfill_above_band(100.000, 95.000, 95.000)]
    #[case::overfill_within_band(100.000, 100.005, 100.000)]
    #[case::overfill_above_band(100.000, 100.050, 100.050)]
    fn test_snap_fill_qty_at_lower_precision(
        #[case] submitted: f64,
        #[case] fill: f64,
        #[case] expected: f64,
    ) {
        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("order-1");
        tracker.register(
            venue_order_id,
            Quantity::new(submitted, 3),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            3,
            2,
        );

        let snapped = tracker.snap_fill_qty(&venue_order_id, Quantity::new(fill, 3));
        assert_eq!(snapped, Quantity::new(expected, 3));
    }

    #[rstest]
    fn test_snap_fill_qty_unregistered_order() {
        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("unknown");
        let fill_qty = Quantity::new(50.0, 6);
        let result = tracker.snap_fill_qty(&venue_order_id, fill_qty);
        assert_eq!(result, fill_qty);
    }

    #[rstest]
    fn test_quote_growth_preserves_provider_quantity_for_budget_validation() {
        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("quote-growth-no-snap");
        tracker.register_without_draining(
            venue_order_id,
            Quantity::new(100.0, 6),
            FillGrowthPolicy::quote_immediate_buy(dec!(100.0)),
        );

        let provider_qty = Quantity::new(100.005, 6);

        assert_eq!(
            tracker.snap_fill_qty(&venue_order_id, provider_qty),
            provider_qty
        );
    }

    #[rstest]
    #[case::quantity(true)]
    #[case::price(false)]
    fn test_applied_trade_replay_requires_identical_economics(#[case] change_quantity: bool) {
        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("economic-replay");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        tracker.register_without_draining(
            venue_order_id,
            Quantity::new(20.0, 6),
            FillGrowthPolicy::Fixed,
        );
        let original = test_fill_report(
            instrument_id,
            venue_order_id,
            "trade-economic-replay",
            Quantity::new(8.0, 6),
        );
        tracker
            .accept_or_buffer_fill(
                venue_order_id,
                original.clone(),
                FillCorrectionMetadata {
                    correction_key: "economic-replay".to_string(),
                    raw_trade_id: "trade-economic-replay".to_string(),
                    raw_corrective_timestamp: "1700000000000".to_string(),
                    info: None,
                    is_confirmed: false,
                },
                |_| Ok(true),
            )
            .unwrap()
            .expect("first fill should be admitted");

        let mut changed = original;
        if change_quantity {
            changed.last_qty = Quantity::new(9.0, 6);
        } else {
            changed.last_px = Price::new(0.56, 2);
        }
        let error = tracker
            .validate_confirmed_fills(&[], &[changed])
            .expect_err("changed replay must fail closed");

        assert!(error.to_string().contains("different fill economics"));
        assert_eq!(
            tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::new(8.0, 6)),
        );
    }

    #[rstest]
    fn test_restored_quote_order_preserves_provider_quantity_but_denies_growth_without_budget() {
        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("restored-no-quote-proof");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        tracker
            .restore_order(
                venue_order_id,
                Quantity::new(10.0, 6),
                Quantity::zero(6),
                FillGrowthPolicy::QuoteImmediateBuyUnproven,
                vec![],
            )
            .unwrap();
        assert_eq!(
            tracker.snap_fill_qty(&venue_order_id, Quantity::new(10.005, 6)),
            Quantity::new(10.005, 6),
        );
        let report = test_fill_report(
            instrument_id,
            venue_order_id,
            "trade-restored-no-quote-proof",
            Quantity::new(11.0, 6),
        );

        let error = tracker
            .accept_or_buffer_fills(
                vec![(
                    venue_order_id,
                    report,
                    FillCorrectionMetadata {
                        correction_key: "restored-no-quote-proof".to_string(),
                        raw_trade_id: "trade-restored-no-quote-proof".to_string(),
                        raw_corrective_timestamp: "1700000000000".to_string(),
                        info: None,
                        is_confirmed: false,
                    },
                )],
                |_| Ok(Some(())),
            )
            .expect_err("restart without a signed quote budget must fail closed");

        assert!(error.to_string().contains("exceeds submitted quantity"));
        assert_eq!(
            tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::zero(6))
        );
    }

    // Verifies the batch helper used by REST callers (`generate_fill_reports`,
    // `generate_mass_status`) snaps each report's `last_qty` and leaves
    // unregistered reports alone. Commission is intentionally untouched.
    #[rstest]
    fn test_snap_fill_reports_snaps_each_in_place() {
        use nautilus_model::{
            enums::LiquiditySide, identifiers::TradeId, reports::FillReport, types::Money,
        };

        let tracker = OrderFillTrackerMap::new();
        let known_id = VenueOrderId::from("known");
        let unknown_id = VenueOrderId::from("unknown");
        tracker.register(
            known_id,
            Quantity::new(714.285710, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        let make_report =
            |venue_order_id: VenueOrderId, last_qty: f64, commission: Decimal| FillReport {
                account_id: AccountId::from("POLY-001"),
                instrument_id: InstrumentId::from("TEST.POLYMARKET"),
                venue_order_id,
                trade_id: TradeId::from("trade"),
                order_side: OrderSide::Buy,
                last_qty: Quantity::new(last_qty, 6),
                last_px: Price::new(0.55, 2),
                commission: Money::from_decimal(commission, pusd()).unwrap(),
                liquidity_side: LiquiditySide::Taker,
                avg_px: None,
                report_id: UUID4::new(),
                ts_event: UnixNanos::default(),
                ts_init: UnixNanos::default(),
                client_order_id: None,
                venue_position_id: None,
            };

        // Known order: 4-ulp overfill, within band, last_qty must snap down.
        // Unknown order: tracker has no entry, reports pass through unchanged.
        let mut reports = vec![
            make_report(known_id, 714.285714, dec!(1.234)),
            make_report(unknown_id, 999.0, dec!(5.678)),
        ];

        tracker.snap_fill_reports(&mut reports);

        assert_eq!(reports[0].last_qty, Quantity::new(714.285710, 6));
        // Commission untouched even though qty was snapped: it tracks venue truth.
        assert_eq!(reports[0].commission.as_decimal(), dec!(1.234));
        assert_eq!(reports[1].last_qty, Quantity::new(999.0, 6));
        assert_eq!(reports[1].commission.as_decimal(), dec!(5.678));
    }

    #[rstest]
    fn test_record_fill_accumulates() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(100.0, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        tracker.record_fill(&vid, Quantity::new(50.0, 6));
        tracker.record_fill(&vid, Quantity::new(49.997714, 6));

        let normalized = tracker.check_terminal_quantity_normalization(&vid);

        assert_eq!(normalized, Some(Quantity::new(99.997714, 6)));
    }

    #[rstest]
    fn test_check_dust_no_residual() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(100.0, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        // Exact fill
        tracker.record_fill(&vid, Quantity::new(100.0, 6));

        assert!(
            tracker
                .check_terminal_quantity_normalization(&vid)
                .is_none()
        );
    }

    #[rstest]
    fn test_check_dust_significant_residual() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(100.0, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        // Only half filled, residual = 50 >> 0.01
        tracker.record_fill(&vid, Quantity::new(50.0, 6));

        assert!(
            tracker
                .check_terminal_quantity_normalization(&vid)
                .is_none()
        );
    }

    #[rstest]
    fn test_take_terminal_ioc_remainder_is_exact_and_idempotent() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-partial-ioc");
        tracker.register(
            vid,
            Quantity::from("30.000000"),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            3,
        );
        tracker.record_fill(&vid, Quantity::from("20.000000"));

        let remainder = tracker.take_terminal_ioc_remainder(&vid);

        assert_eq!(remainder, Some(Quantity::from("10.000000")));
        assert!(!tracker.contains(&vid));
        assert!(tracker.take_terminal_ioc_remainder(&vid).is_none());
    }

    #[rstest]
    fn test_take_terminal_ioc_remainder_requires_a_partial_fill() {
        let tracker = OrderFillTrackerMap::new();
        let unfilled = VenueOrderId::from("order-unfilled-ioc");
        let filled = VenueOrderId::from("order-filled-ioc");
        for venue_order_id in [unfilled, filled] {
            tracker.register(
                venue_order_id,
                Quantity::from("20.000000"),
                OrderSide::Buy,
                InstrumentId::from("TEST.POLYMARKET"),
                6,
                3,
            );
        }
        tracker.record_fill(&filled, Quantity::from("20.000000"));

        assert!(tracker.take_terminal_ioc_remainder(&unfilled).is_none());
        assert!(tracker.take_terminal_ioc_remainder(&filled).is_none());
        assert!(tracker.contains(&unfilled));
        assert!(tracker.contains(&filled));
    }

    #[rstest]
    fn test_check_dust_unregistered() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("unknown");

        assert!(
            tracker
                .check_terminal_quantity_normalization(&vid)
                .is_none()
        );
    }

    #[rstest]
    fn test_dust_settlement_removes_entry() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(100.0, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        tracker.record_fill(&vid, Quantity::new(99.995, 6));

        let normalized = tracker.check_terminal_quantity_normalization(&vid);
        assert_eq!(normalized, Some(Quantity::new(99.995, 6)));

        // Entry should be removed, second check returns None (no duplicate).
        assert!(!tracker.contains(&vid));
        assert!(
            tracker
                .check_terminal_quantity_normalization(&vid)
                .is_none()
        );
    }

    #[rstest]
    fn test_get_cumulative_filled_no_fills() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(100.0, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        let filled = tracker.get_cumulative_filled(&vid);
        assert_eq!(filled, Some(Quantity::zero(6)));
    }

    #[rstest]
    fn test_get_cumulative_filled_with_fills() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(100.0, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        tracker.record_fill(&vid, Quantity::new(30.0, 6));
        tracker.record_fill(&vid, Quantity::new(20.0, 6));

        let filled = tracker.get_cumulative_filled(&vid);
        assert_eq!(filled, Some(Quantity::new(50.0, 6)));
    }

    #[rstest]
    fn test_get_cumulative_filled_unregistered() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("unknown");
        assert!(tracker.get_cumulative_filled(&vid).is_none());
    }

    #[rstest]
    fn test_is_fully_filled_unregistered() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("unknown");
        assert!(!tracker.is_fully_filled(&vid));
    }

    #[rstest]
    fn test_is_fully_filled_partial() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(100.0, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        tracker.record_fill(&vid, Quantity::new(50.0, 6));
        assert!(!tracker.is_fully_filled(&vid));
    }

    #[rstest]
    fn test_is_fully_filled_complete() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(100.0, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        tracker.record_fill(&vid, Quantity::new(60.0, 6));
        tracker.record_fill(&vid, Quantity::new(40.0, 6));
        assert!(tracker.is_fully_filled(&vid));
    }
}
