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
use nautilus_core::MUTEX_POISONED;
use nautilus_model::{
    enums::OrderSide,
    events::OrderFilled,
    identifiers::{ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    reports::{FillReport, OrderStatusReport},
    types::Quantity,
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::identity::OrderIdentity;
use crate::common::consts::DUST_SNAP_THRESHOLD_DEC;

/// Cumulative fill state for a single order.
#[derive(Debug, Clone, Copy)]
struct OrderFillState {
    submitted_qty: Quantity,
    cumulative_filled: Quantity,
    order_side: OrderSide,
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
    pub reports: Vec<Option<(FillReport, T)>>,
    pub binding_error: Option<anyhow::Error>,
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
    confirmed_trades: FifoCache<String, 10_000>,
    applied_buffered_fills: FifoCacheMap<String, Vec<OrderFilled>, 10_000>,
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
        order_side: OrderSide,
    ) {
        let mut state = new_order_state(submitted_qty, order_side);
        state.cumulative_filled = filled_qty;
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .orders
            .insert(venue_order_id, state);
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
            });
        if let Some(error) = outcome.binding_error {
            return Err(error);
        }
        Ok(outcome.reports.pop().flatten().map(|(report, ())| report))
    }

    /// Admits one native correction batch only after every participant binding is checked.
    ///
    /// A binding error retains the entire untouched batch. A replay matching already-pending
    /// correction participants is also retained without duplication, so the eventual registered
    /// order drain remains the only authority transition for that evidence.
    pub(crate) fn accept_or_buffer_fills<T, F>(
        &self,
        fills: Vec<(VenueOrderId, FillReport, FillCorrectionMetadata)>,
        mut reversible_target: F,
    ) -> FillBatchAdmission<T>
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
                return FillBatchAdmission {
                    reports: (0..report_count).map(|_| None).collect(),
                    binding_error: Some(anyhow::anyhow!(
                        "duplicate correction participant {} for order {} and trade {}",
                        correction.correction_key,
                        venue_order_id,
                        report.trade_id
                    )),
                };
            }
        }

        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        let already_pending = fills
            .iter()
            .map(|(_, report, correction)| {
                pending_correction_participant(&guard, report, correction)
            })
            .collect::<Vec<_>>();

        if already_pending.iter().any(|pending| *pending) {
            return FillBatchAdmission {
                reports: (0..report_count).map(|_| None).collect(),
                binding_error: None,
            };
        }

        let mut decisions = Vec::with_capacity(fills.len());
        for (_, report, _) in &fills {
            match reversible_target(report) {
                Ok(target) => decisions.push(target),
                Err(error) => {
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
                    return FillBatchAdmission {
                        reports: (0..report_count).map(|_| None).collect(),
                        binding_error: Some(error),
                    };
                }
            }
        }

        let reports = fills
            .into_iter()
            .zip(decisions)
            .map(
                |((venue_order_id, report, correction), target)| match target {
                    Some(target) if guard.orders.get(&venue_order_id).is_some() => {
                        record_fill_in(&mut guard.orders, &venue_order_id, report.last_qty);
                        Some((report, target))
                    }
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
        FillBatchAdmission {
            reports,
            binding_error: None,
        }
    }

    /// Registers an accepted order while retaining fills until reversible identity is available.
    pub(crate) fn register_without_draining(
        &self,
        venue_order_id: VenueOrderId,
        submitted_qty: Quantity,
        order_side: OrderSide,
    ) {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .orders
            .insert(venue_order_id, new_order_state(submitted_qty, order_side));
    }

    /// Returns a tracked order report to emit, or buffers it until the order is registered.
    ///
    /// The accepted-check and the buffer insert run under one lock, so the submit path's register
    /// (sequenced before its report drain) cannot leave the report buffered with no later drain.
    /// Returns the report to emit when the order is registered, or `None` when it was buffered.
    pub(crate) fn accept_or_buffer_report(
        &self,
        venue_order_id: VenueOrderId,
        report: OrderStatusReport,
    ) -> Option<OrderStatusReport> {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        if guard.orders.get(&venue_order_id).is_some() {
            Some(report)
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

    /// Emits retained reports only when no earlier fill is awaiting binding.
    ///
    /// The pending-fill check, report removal, and callback are one critical section. The
    /// callback must not call back into this tracker.
    pub(crate) fn emit_pending_reports_if_no_pending_fill<F>(
        &self,
        venue_order_id: &VenueOrderId,
        emit: F,
    ) -> bool
    where
        F: FnOnce(&[OrderStatusReport]),
    {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        if guard
            .pending_fills
            .get(venue_order_id)
            .is_some_and(|fills| !fills.is_empty())
        {
            return false;
        }
        let reports = guard
            .pending_reports
            .remove(venue_order_id)
            .unwrap_or_default();
        emit(&reports);
        true
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
            new_order_state(submitted_qty, identity.order_side),
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

    /// Returns a snapshot of buffered order reports without removing them.
    pub(crate) fn pending_reports_for(
        &self,
        venue_order_id: &VenueOrderId,
    ) -> Vec<OrderStatusReport> {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .pending_reports
            .get(venue_order_id)
            .cloned()
            .unwrap_or_default()
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
            .cloned()
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
        guard
            .applied_buffered_fills
            .get(&correction_key.to_string())
            .is_some_and(|fills| !fills.is_empty())
    }

    /// Marks a trade failed and returns buffered fills that were already emitted.
    pub(crate) fn void_buffered_trade(&self, correction_key: &str) -> Vec<OrderFilled> {
        let key = correction_key.to_string();
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        guard.confirmed_trades.remove(&key);
        guard.voided_trades.add(key.clone());
        let fills = guard
            .applied_buffered_fills
            .remove(&key)
            .unwrap_or_default();

        for fill in &fills {
            reverse_fill_in(&mut guard.orders, &fill.venue_order_id, fill.last_qty);
        }
        fills
    }

    pub(crate) fn mark_trade_confirmed(&self, correction_key: &str) {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .confirmed_trades
            .add(correction_key.to_string());
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn is_trade_confirmed(&self, correction_key: &str) -> bool {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .confirmed_trades
            .contains(&correction_key.to_string())
    }

    pub(crate) fn reverse_fill(&self, venue_order_id: &VenueOrderId, quantity: Quantity) {
        reverse_fill_in(
            &mut self.inner.lock().expect(MUTEX_POISONED).orders,
            venue_order_id,
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

    /// Raise the registered quantity to the cumulative BUY fills when they exceed it, returning
    /// the new quantity to emit via `OrderUpdated` (or `None` when no raise is needed).
    ///
    /// A Polymarket BUY is bounded by the USDC it spends (`makerAmount`), so a marketable fill
    /// below the limit price returns more shares than the nominal quantity. The engine rejects a
    /// fill past the order quantity, so the quantity is raised to the actual fill before the
    /// `OrderFilled` applies. SELL orders are share-denominated and never overfill, so they always
    /// return `None`. Dust overfills are handled earlier by `snap_fill_qty`, so only a gross
    /// overfill reaches here.
    ///
    /// Raising `submitted_qty` to exactly the cumulative fill makes the following `OrderFilled`
    /// reach `Filled`. That is correct because an overfill only ever occurs on a marketable taker
    /// BUY, whose fill the venue reports as a single aggregated trade event (one `FillReport` per
    /// taker order): the bumping fill is therefore terminal, with no later fill to strand. Passive
    /// maker BUYs can fill across several events but execute at their own price, so they never
    /// overfill and never reach this raise. A venue that split one marketable BUY across multiple
    /// trade events would close the order on the first crossing fill; this is not Polymarket's
    /// observed behaviour and would need a final-fill signal to handle.
    pub(crate) fn buy_overfill_bump(&self, venue_order_id: &VenueOrderId) -> Option<Quantity> {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        buy_overfill_bump_in(&mut guard.orders, venue_order_id)
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

fn new_order_state(submitted_qty: Quantity, order_side: OrderSide) -> OrderFillState {
    OrderFillState {
        submitted_qty,
        cumulative_filled: Quantity::zero(submitted_qty.precision),
        order_side,
    }
}

fn pending_correction_participant(
    inner: &TrackerInner,
    report: &FillReport,
    correction: &FillCorrectionMetadata,
) -> bool {
    inner
        .pending_fills
        .values()
        .flat_map(|fills| fills.iter())
        .any(|pending| {
            pending.report.venue_order_id == report.venue_order_id
                && pending.report.trade_id == report.trade_id
                && pending
                    .correction
                    .as_ref()
                    .is_some_and(|metadata| metadata.correction_key == correction.correction_key)
        })
}

fn buy_overfill_bump_in(
    orders: &mut AHashMap<VenueOrderId, OrderFillState>,
    venue_order_id: &VenueOrderId,
) -> Option<Quantity> {
    let state = orders.get_mut(venue_order_id)?;
    if state.order_side != OrderSide::Buy {
        return None;
    }

    if state.cumulative_filled > state.submitted_qty {
        state.submitted_qty = state.cumulative_filled;
        Some(state.cumulative_filled)
    } else {
        None
    }
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

/// Drains the buffered fills for `venue_order_id`, validates their eventual order binding, and
/// stamps the client order ID. The caller must hold the lock and have registered the order first.
fn take_and_prepare_fills(
    inner: &mut TrackerInner,
    venue_order_id: VenueOrderId,
    client_order_id: Option<ClientOrderId>,
    instrument_id: InstrumentId,
    order_side: OrderSide,
) -> anyhow::Result<Vec<BufferedFill>> {
    validate_pending_fill_binding(inner, venue_order_id, instrument_id, order_side)?;
    let Some(buffered) = inner.pending_fills.remove(&venue_order_id) else {
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
    let fills = take_and_prepare_fills(
        inner,
        venue_order_id,
        client_order_id,
        instrument_id,
        order_side,
    )?;
    before_emit(&fills);
    let mut emissions = Vec::with_capacity(fills.len());
    for buffered in fills {
        if let Some(correction) = buffered.correction.as_ref()
            && inner.voided_trades.contains(&correction.correction_key)
        {
            emissions.push(BufferedFillEmission {
                buffered,
                emitted: false,
            });
            continue;
        }

        record_fill_in(
            &mut inner.orders,
            &buffered.report.venue_order_id,
            buffered.report.last_qty,
        );
        let new_qty = buy_overfill_bump_in(&mut inner.orders, &buffered.report.venue_order_id);
        let filled = emit(&buffered, new_qty);
        if let Some(correction) = buffered.correction.as_ref() {
            if let Some(fills) = inner
                .applied_buffered_fills
                .get_mut(&correction.correction_key)
            {
                fills.push(filled);
            } else {
                inner
                    .applied_buffered_fills
                    .insert(correction.correction_key.clone(), vec![filled]);
            }
            if correction.is_confirmed {
                inner
                    .confirmed_trades
                    .add(correction.correction_key.clone());
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
        order_side: OrderSide,
        _instrument_id: InstrumentId,
        _size_precision: u8,
        _price_precision: u8,
    ) {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .orders
            .insert(venue_order_id, new_order_state(submitted_qty, order_side));
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
    qty: Quantity,
) {
    if let Some(state) = orders.get_mut(venue_order_id) {
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
        let prior_fills = tracker.void_buffered_trade(correction_key);
        tracker.register_without_draining(venue_order_id, Quantity::new(10.0, 6), OrderSide::Buy);
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
        assert!(tracker.void_buffered_trade("trade-voided-order").is_empty());
        tracker.register_without_draining(venue_order_id, Quantity::new(100.0, 6), OrderSide::Buy);

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
        tracker.register_without_draining(venue_order_id, Quantity::new(100.0, 6), OrderSide::Buy);

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
                OrderSide::Buy,
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
        tracker.register_without_draining(venue_order_id, Quantity::new(100.0, 6), OrderSide::Buy);

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

        let first = tracker.accept_or_buffer_fills(
            vec![(venue_order_id, report.clone(), correction.clone())],
            |_| -> anyhow::Result<Option<()>> {
                anyhow::bail!("identity changed before the atomic decision")
            },
        );
        assert!(first.binding_error.is_some());
        assert!(first.reports.iter().all(Option::is_none));
        assert_eq!(tracker.pending_fills_for(&venue_order_id).len(), 1);

        tracker.register_without_draining(venue_order_id, Quantity::new(100.0, 6), OrderSide::Buy);
        let replay = tracker
            .accept_or_buffer_fills(vec![(venue_order_id, report, correction)], |_| Ok(Some(())));
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
                OrderSide::Buy,
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

        let outcome = tracker.accept_or_buffer_fills(
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
        );

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
    fn test_partial_batch_replay_does_not_buffer_already_emitted_sibling() {
        use std::cell::Cell;

        let tracker = OrderFillTrackerMap::new();
        let first_order = VenueOrderId::from("maker-order-replay-applied");
        let second_order = VenueOrderId::from("maker-order-replay-pending");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        tracker.register_without_draining(first_order, Quantity::new(100.0, 6), OrderSide::Buy);
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

        let first = tracker.accept_or_buffer_fills(batch(), |_| Ok(Some(())));
        assert!(first.binding_error.is_none());
        assert!(first.reports[0].is_some());
        assert!(first.reports[1].is_none());
        assert!(!tracker.has_pending_fill(&first_order));
        assert!(tracker.has_pending_fill(&second_order));
        assert_eq!(
            tracker.get_cumulative_filled(&first_order),
            Some(Quantity::new(10.0, 6))
        );

        let replay = tracker.accept_or_buffer_fills(batch(), |_| Ok(Some(())));
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

    fn register_buy(tracker: &OrderFillTrackerMap, vid: VenueOrderId, submitted: f64) {
        tracker.register(
            vid,
            Quantity::new(submitted, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );
    }

    #[rstest]
    fn test_buy_overfill_bump_unregistered_is_none() {
        let tracker = OrderFillTrackerMap::new();
        assert!(
            tracker
                .buy_overfill_bump(&VenueOrderId::from("unknown"))
                .is_none()
        );
    }

    #[rstest]
    fn test_buy_overfill_bump_within_qty_is_none() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        register_buy(&tracker, vid, 10.0);

        // Exact fill: cumulative equals submitted, no raise.
        tracker.record_fill(&vid, Quantity::new(10.0, 6));
        assert!(tracker.buy_overfill_bump(&vid).is_none());
    }

    #[rstest]
    fn test_buy_overfill_bump_sell_is_none() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(10.0, 6),
            OrderSide::Sell,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        // A SELL is share-denominated; even an over-report does not raise the quantity.
        tracker.record_fill(&vid, Quantity::new(12.0, 6));
        assert!(tracker.buy_overfill_bump(&vid).is_none());
    }

    #[rstest]
    fn test_buy_overfill_bump_raises_to_cumulative_and_is_idempotent() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        register_buy(&tracker, vid, 10.0);

        // Marketable BUY fills below its limit: 12 shares against a nominal 10.
        tracker.record_fill(&vid, Quantity::new(12.0, 6));

        let bumped = tracker.buy_overfill_bump(&vid).expect("expected a raise");
        assert_eq!(bumped, Quantity::new(12.0, 6));
        // Submitted is raised, so a second emit for the same fill does not re-raise.
        assert!(tracker.buy_overfill_bump(&vid).is_none());
        // Leaves are non-negative after the raise, so no spurious dust residual.
        assert!(tracker.is_fully_filled(&vid));
    }

    #[rstest]
    fn test_buy_overfill_bump_tracks_each_crossing_fill() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        register_buy(&tracker, vid, 10.0);

        // First partial stays within the nominal qty: no raise.
        tracker.record_fill(&vid, Quantity::new(6.0, 6));
        assert!(tracker.buy_overfill_bump(&vid).is_none());

        // Second partial crosses the nominal qty: raise to cumulative 14.
        tracker.record_fill(&vid, Quantity::new(8.0, 6));
        assert_eq!(
            tracker.buy_overfill_bump(&vid),
            Some(Quantity::new(14.0, 6))
        );

        // Third partial crosses again: raise to cumulative 20.
        tracker.record_fill(&vid, Quantity::new(6.0, 6));
        assert_eq!(
            tracker.buy_overfill_bump(&vid),
            Some(Quantity::new(20.0, 6))
        );
    }

    // A dust overfill is snapped DOWN by snap_fill_qty before recording, so it never reaches
    // buy_overfill_bump as a raise: the two mechanisms do not double-handle the same fill.
    #[rstest]
    fn test_buy_overfill_bump_ignores_dust_snapped_fill() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        register_buy(&tracker, vid, 100.0);

        let raw = Quantity::new(100.005, 6);
        let snapped = tracker.snap_fill_qty(&vid, raw);
        assert_eq!(snapped, Quantity::new(100.0, 6));

        tracker.record_fill(&vid, snapped);
        assert!(tracker.buy_overfill_bump(&vid).is_none());
    }
}
