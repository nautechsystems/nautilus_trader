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

//! Per-order fill tracking with dust detection for the Polymarket adapter.

use std::sync::Mutex;

use nautilus_common::cache::fifo::FifoCacheMap;
use nautilus_core::{MUTEX_POISONED, UUID4, UnixNanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide},
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};

use crate::common::consts::DUST_SNAP_THRESHOLD;

/// Cumulative fill state for a single order.
#[derive(Debug, Clone, Copy)]
struct OrderFillState {
    submitted_qty: Quantity,
    cumulative_filled: f64,
    last_fill_px: f64,
    last_fill_ts: UnixNanos,
    order_side: OrderSide,
    instrument_id: InstrumentId,
    size_precision: u8,
    price_precision: u8,
}

/// Registration map plus the fill and order-report buffers, all under one mutex.
///
/// Co-locating the buffers with the registration map is what closes the buffer-after-drain race:
/// the WS dispatch's accepted-check and buffer, and the submit path's register and drain, are all
/// single critical sections on this one lock, so a buffer can never slip between a register and the
/// drain that follows it.
#[derive(Debug, Default)]
struct TrackerInner {
    orders: FifoCacheMap<VenueOrderId, OrderFillState, 10_000>,
    pending_fills: FifoCacheMap<VenueOrderId, Vec<FillReport>, 1_000>,
    pending_reports: FifoCacheMap<VenueOrderId, Vec<OrderStatusReport>, 1_000>,
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
            Some(s) => s.cumulative_filled > 0.0,
            None => true, // Removed = already settled
        }
    }

    /// Returns the cumulative filled quantity for an order, if tracked.
    pub(crate) fn get_cumulative_filled(&self, venue_order_id: &VenueOrderId) -> Option<f64> {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .orders
            .get(venue_order_id)
            .map(|s| s.cumulative_filled)
    }

    /// Returns `true` if cumulative fills have reached the submitted quantity
    /// (within a tight tolerance to account for f64 accumulation noise).
    pub(crate) fn is_fully_filled(&self, venue_order_id: &VenueOrderId) -> bool {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .orders
            .get(venue_order_id)
            .is_some_and(|s| {
                let leaves = s.submitted_qty.as_f64() - s.cumulative_filled;
                leaves < 1e-9
            })
    }

    /// Records a tracked fill, or buffers it until the order is registered, atomically.
    ///
    /// The accepted-check and the buffer insert run under one lock, so the submit path's register
    /// and drain (the same lock) cannot interleave between them. Returns the report to emit when the
    /// order is registered, or `None` when it was buffered.
    pub(crate) fn accept_or_buffer_fill(
        &self,
        venue_order_id: VenueOrderId,
        report: FillReport,
    ) -> Option<FillReport> {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        if guard.orders.get(&venue_order_id).is_some() {
            record_fill_in(
                &mut guard.orders,
                &venue_order_id,
                report.last_qty.as_f64(),
                report.last_px.as_f64(),
                report.ts_event,
            );
            Some(report)
        } else {
            push_buffered(&mut guard.pending_fills, venue_order_id, report);
            None
        }
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

    /// Registers the order, then drains and prepares its buffered fills under one lock.
    ///
    /// Registration and the drain are a single critical section, so a concurrent
    /// [`Self::accept_or_buffer_fill`] cannot read the order as unregistered and buffer a fill into
    /// the window after this drain.
    #[expect(clippy::too_many_arguments)]
    pub(crate) fn register_and_take_pending_fills(
        &self,
        venue_order_id: VenueOrderId,
        client_order_id: Option<ClientOrderId>,
        submitted_qty: Quantity,
        order_side: OrderSide,
        instrument_id: InstrumentId,
        size_precision: u8,
        price_precision: u8,
    ) -> Vec<FillReport> {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        guard.orders.insert(
            venue_order_id,
            new_order_state(
                submitted_qty,
                order_side,
                instrument_id,
                size_precision,
                price_precision,
            ),
        );
        take_and_prepare_fills(&mut guard, venue_order_id, client_order_id)
    }

    /// Registers the order and drains its buffered fills only when a fill is already buffered.
    ///
    /// Used by the unknown-submit path, where acceptance is deferred until a buffered fill proves
    /// the venue took the order. Returns `None` (registering nothing) when no fill is buffered.
    #[expect(clippy::too_many_arguments)]
    pub(crate) fn register_and_take_pending_fills_if_buffered(
        &self,
        venue_order_id: VenueOrderId,
        client_order_id: Option<ClientOrderId>,
        submitted_qty: Quantity,
        order_side: OrderSide,
        instrument_id: InstrumentId,
        size_precision: u8,
        price_precision: u8,
    ) -> Option<Vec<FillReport>> {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        if !guard.pending_fills.contains_key(&venue_order_id) {
            return None;
        }
        guard.orders.insert(
            venue_order_id,
            new_order_state(
                submitted_qty,
                order_side,
                instrument_id,
                size_precision,
                price_precision,
            ),
        );
        Some(take_and_prepare_fills(
            &mut guard,
            venue_order_id,
            client_order_id,
        ))
    }

    /// Drains and prepares buffered fills for an already-registered order.
    pub(crate) fn take_pending_fills(
        &self,
        venue_order_id: VenueOrderId,
        client_order_id: Option<ClientOrderId>,
    ) -> Vec<FillReport> {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        take_and_prepare_fills(&mut guard, venue_order_id, client_order_id)
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
    /// dust overfill (within `DUST_SNAP_THRESHOLD`).
    ///
    /// Overfill snapping is required because the engine rejects fills past
    /// `submitted_qty`. Underfill is intentionally left alone here: a single
    /// partial fill that happens to land near submitted_qty might still be
    /// followed by additional matches, or the order might end up canceled
    /// with the dust remaining as legitimate leaves. The
    /// `check_dust_and_build_fill` synthetic-fill mechanism handles the CLOB
    /// cent-tick truncation case at MATCHED status, where the order's
    /// terminal state is known.
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
        let state = guard.orders.get_mut(venue_order_id)?;
        if state.order_side != OrderSide::Buy {
            return None;
        }

        let filled = Quantity::new(state.cumulative_filled, state.size_precision);
        if filled > state.submitted_qty {
            state.submitted_qty = filled;
            Some(filled)
        } else {
            None
        }
    }

    /// Check if an order has a dust residual after all fills.
    /// Returns `Some(FillReport)` if a synthetic fill should be emitted.
    /// Removes the entry on dust settlement to prevent duplicate synthetic
    /// fills from repeated MATCHED events.
    #[expect(clippy::too_many_arguments)]
    pub(crate) fn check_dust_and_build_fill(
        &self,
        venue_order_id: &VenueOrderId,
        account_id: AccountId,
        order_id: &str,
        fallback_px: f64,
        currency: Currency,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<FillReport> {
        let mut guard = self.inner.lock().expect(MUTEX_POISONED);
        let s = guard.orders.get(venue_order_id)?;
        let leaves = s.submitted_qty.as_f64() - s.cumulative_filled;

        if leaves > 0.0 && leaves < DUST_SNAP_THRESHOLD {
            // Copy fields before removing the entry
            let size_precision = s.size_precision;
            let price_precision = s.price_precision;
            let last_fill_px = s.last_fill_px;
            let order_side = s.order_side;
            let instrument_id = s.instrument_id;

            log::debug!(
                "Order {venue_order_id} MATCHED with dust residual {leaves:.6} -- \
                 emitting synthetic fill to reach FILLED"
            );
            let dust_qty = Quantity::new(leaves, size_precision);
            let px = if last_fill_px > 0.0 {
                last_fill_px
            } else {
                fallback_px
            };
            let fill_px = Price::new(px, price_precision);
            let trade_id = TradeId::from(format!("{order_id:.27}-dust").as_str());

            // Remove entry: order is settled, prevents duplicate dust fills
            guard.orders.remove(venue_order_id);

            Some(FillReport {
                account_id,
                instrument_id,
                venue_order_id: *venue_order_id,
                trade_id,
                order_side,
                last_qty: dust_qty,
                last_px: fill_px,
                commission: Money::zero(currency),
                liquidity_side: LiquiditySide::NoLiquiditySide,
                avg_px: None,
                report_id: UUID4::new(),
                ts_event,
                ts_init,
                client_order_id: None,
                venue_position_id: None,
            })
        } else {
            if leaves >= DUST_SNAP_THRESHOLD {
                log::debug!(
                    "Order {venue_order_id} MATCHED with significant residual \
                     {leaves:.6} (filled {}/{})",
                    s.cumulative_filled,
                    s.submitted_qty,
                );
            }
            None
        }
    }
}

fn new_order_state(
    submitted_qty: Quantity,
    order_side: OrderSide,
    instrument_id: InstrumentId,
    size_precision: u8,
    price_precision: u8,
) -> OrderFillState {
    OrderFillState {
        submitted_qty,
        cumulative_filled: 0.0,
        last_fill_px: 0.0,
        last_fill_ts: UnixNanos::default(),
        order_side,
        instrument_id,
        size_precision,
        price_precision,
    }
}

/// Drains the buffered fills for `venue_order_id`, stamping the client order ID and snapping and
/// recording each one. The caller must hold the lock and have registered the order first.
fn take_and_prepare_fills(
    inner: &mut TrackerInner,
    venue_order_id: VenueOrderId,
    client_order_id: Option<ClientOrderId>,
) -> Vec<FillReport> {
    let Some(buffered) = inner.pending_fills.remove(&venue_order_id) else {
        return Vec::new();
    };
    buffered
        .into_iter()
        .map(|mut fill| {
            fill.client_order_id = client_order_id;
            fill.last_qty = snap_fill_qty_in(&inner.orders, &venue_order_id, fill.last_qty);
            record_fill_in(
                &mut inner.orders,
                &venue_order_id,
                fill.last_qty.as_f64(),
                fill.last_px.as_f64(),
                fill.ts_event,
            );
            fill
        })
        .collect()
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
        instrument_id: InstrumentId,
        size_precision: u8,
        price_precision: u8,
    ) {
        self.inner.lock().expect(MUTEX_POISONED).orders.insert(
            venue_order_id,
            new_order_state(
                submitted_qty,
                order_side,
                instrument_id,
                size_precision,
                price_precision,
            ),
        );
    }

    /// Records a fill against a registered order, for tests that drive fill accumulation directly.
    pub(crate) fn record_fill(
        &self,
        venue_order_id: &VenueOrderId,
        qty: f64,
        px: f64,
        ts: UnixNanos,
    ) {
        record_fill_in(
            &mut self.inner.lock().expect(MUTEX_POISONED).orders,
            venue_order_id,
            qty,
            px,
            ts,
        );
    }

    /// Buffers a fill as if it arrived on the WS channel before the order was registered.
    pub(crate) fn buffer_fill_for_test(&self, venue_order_id: VenueOrderId, report: FillReport) {
        push_buffered(
            &mut self.inner.lock().expect(MUTEX_POISONED).pending_fills,
            venue_order_id,
            report,
        );
    }

    /// Buffers an order report as if it arrived on the WS channel before the order was registered.
    pub(crate) fn buffer_report_for_test(
        &self,
        venue_order_id: VenueOrderId,
        report: OrderStatusReport,
    ) {
        push_buffered(
            &mut self.inner.lock().expect(MUTEX_POISONED).pending_reports,
            venue_order_id,
            report,
        );
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
    orders: &mut FifoCacheMap<VenueOrderId, OrderFillState, 10_000>,
    venue_order_id: &VenueOrderId,
    qty: f64,
    px: f64,
    ts: UnixNanos,
) {
    if let Some(s) = orders.get_mut(venue_order_id) {
        s.cumulative_filled += qty;
        s.last_fill_px = px;
        s.last_fill_ts = ts;
    }
}

fn snap_fill_qty_in(
    orders: &FifoCacheMap<VenueOrderId, OrderFillState, 10_000>,
    venue_order_id: &VenueOrderId,
    fill_qty: Quantity,
) -> Quantity {
    match orders.get(venue_order_id) {
        Some(s) => {
            let diff = s.submitted_qty.as_f64() - fill_qty.as_f64();
            if diff < 0.0 && diff.abs() < DUST_SNAP_THRESHOLD {
                log::debug!(
                    "Snapping overfill {fill_qty} -> {} (dust={diff:+.6})",
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
    use rstest::rstest;

    use super::*;

    fn pusd() -> Currency {
        Currency::pUSD()
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

    // snap_fill_qty is overfill-only. Underfill is preserved so partial fills
    // followed by cancel keep their venue-reported size; the synthetic dust
    // fill at MATCHED status handles CLOB cent-tick truncation.
    #[rstest]
    // Underfill within the dust band: NOT snapped. The fill is recorded
    // as-is; if the order reaches MATCHED, the synthetic dust mechanism
    // emits the missing leaves at that point.
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
            |venue_order_id: VenueOrderId, last_qty: f64, commission: f64| FillReport {
                account_id: AccountId::from("POLY-001"),
                instrument_id: InstrumentId::from("TEST.POLYMARKET"),
                venue_order_id,
                trade_id: TradeId::from("trade"),
                order_side: OrderSide::Buy,
                last_qty: Quantity::new(last_qty, 6),
                last_px: Price::new(0.55, 2),
                commission: Money::new(commission, pusd()),
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
            make_report(known_id, 714.285714, 1.234),
            make_report(unknown_id, 999.0, 5.678),
        ];

        tracker.snap_fill_reports(&mut reports);

        assert_eq!(reports[0].last_qty, Quantity::new(714.285710, 6));
        // Commission untouched even though qty was snapped: it tracks venue truth.
        assert_eq!(reports[0].commission, Money::new(1.234, pusd()));
        assert_eq!(reports[1].last_qty, Quantity::new(999.0, 6));
        assert_eq!(reports[1].commission, Money::new(5.678, pusd()));
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

        tracker.record_fill(&vid, 50.0, 0.55, UnixNanos::from(1_000u64));
        tracker.record_fill(&vid, 49.997714, 0.55, UnixNanos::from(2_000u64));

        // Dust check: 100.0 - 99.997714 = 0.002286 < 0.01 -> emit
        let dust_fill = tracker.check_dust_and_build_fill(
            &vid,
            AccountId::from("POLY-001"),
            "order-1",
            0.55,
            pusd(),
            UnixNanos::from(3_000u64),
            UnixNanos::from(4_000u64),
        );
        assert!(dust_fill.is_some());
        let fill = dust_fill.unwrap();
        assert!((fill.last_qty.as_f64() - 0.002286).abs() < 1e-9);
        assert_eq!(fill.order_side, OrderSide::Buy);
        assert_eq!(fill.liquidity_side, LiquiditySide::NoLiquiditySide);
        assert_eq!(fill.ts_event, UnixNanos::from(3_000u64));
        assert_eq!(fill.ts_init, UnixNanos::from(4_000u64));
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
        tracker.record_fill(&vid, 100.0, 0.55, UnixNanos::from(1_000u64));

        let dust_fill = tracker.check_dust_and_build_fill(
            &vid,
            AccountId::from("POLY-001"),
            "order-1",
            0.55,
            pusd(),
            UnixNanos::from(2_000u64),
            UnixNanos::from(2_000u64),
        );
        assert!(dust_fill.is_none());
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
        tracker.record_fill(&vid, 50.0, 0.55, UnixNanos::from(1_000u64));

        let dust_fill = tracker.check_dust_and_build_fill(
            &vid,
            AccountId::from("POLY-001"),
            "order-1",
            0.55,
            pusd(),
            UnixNanos::from(2_000u64),
            UnixNanos::from(2_000u64),
        );
        assert!(dust_fill.is_none());
    }

    #[rstest]
    fn test_check_dust_unregistered() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("unknown");

        let dust_fill = tracker.check_dust_and_build_fill(
            &vid,
            AccountId::from("POLY-001"),
            "unknown",
            0.55,
            pusd(),
            UnixNanos::from(1_000u64),
            UnixNanos::from(1_000u64),
        );
        assert!(dust_fill.is_none());
    }

    #[rstest]
    fn test_dust_fill_uses_last_fill_price() {
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

        tracker.record_fill(&vid, 99.995, 0.60, UnixNanos::from(1_000u64));

        let dust_fill = tracker
            .check_dust_and_build_fill(
                &vid,
                AccountId::from("POLY-001"),
                "order-1",
                0.50, // fallback, should NOT be used
                pusd(),
                UnixNanos::from(2_000u64),
                UnixNanos::from(2_000u64),
            )
            .unwrap();

        // Should use last fill price (0.60), not fallback (0.50)
        assert_eq!(dust_fill.last_px, Price::new(0.60, 2));
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

        tracker.record_fill(&vid, 99.995, 0.55, UnixNanos::from(1_000u64));

        // First check returns dust
        let dust_fill = tracker.check_dust_and_build_fill(
            &vid,
            AccountId::from("POLY-001"),
            "order-1",
            0.55,
            pusd(),
            UnixNanos::from(2_000u64),
            UnixNanos::from(2_000u64),
        );
        assert!(dust_fill.is_some());

        // Entry should be removed, second check returns None (no duplicate)
        assert!(!tracker.contains(&vid));
        let dust_fill2 = tracker.check_dust_and_build_fill(
            &vid,
            AccountId::from("POLY-001"),
            "order-1",
            0.55,
            pusd(),
            UnixNanos::from(3_000u64),
            UnixNanos::from(3_000u64),
        );
        assert!(dust_fill2.is_none());
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
        assert_eq!(filled, Some(0.0));
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

        tracker.record_fill(&vid, 30.0, 0.5, UnixNanos::from(1_000u64));
        tracker.record_fill(&vid, 20.0, 0.5, UnixNanos::from(2_000u64));

        let filled = tracker.get_cumulative_filled(&vid);
        assert_eq!(filled, Some(50.0));
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

        tracker.record_fill(&vid, 50.0, 0.5, UnixNanos::from(1_000u64));
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

        tracker.record_fill(&vid, 60.0, 0.5, UnixNanos::from(1_000u64));
        tracker.record_fill(&vid, 40.0, 0.5, UnixNanos::from(2_000u64));
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
        tracker.record_fill(&vid, 10.0, 0.5, UnixNanos::from(1_000u64));
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
        tracker.record_fill(&vid, 12.0, 0.5, UnixNanos::from(1_000u64));
        assert!(tracker.buy_overfill_bump(&vid).is_none());
    }

    #[rstest]
    fn test_buy_overfill_bump_raises_to_cumulative_and_is_idempotent() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        register_buy(&tracker, vid, 10.0);

        // Marketable BUY fills below its limit: 12 shares against a nominal 10.
        tracker.record_fill(&vid, 12.0, 0.5, UnixNanos::from(1_000u64));

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
        tracker.record_fill(&vid, 6.0, 0.5, UnixNanos::from(1_000u64));
        assert!(tracker.buy_overfill_bump(&vid).is_none());

        // Second partial crosses the nominal qty: raise to cumulative 14.
        tracker.record_fill(&vid, 8.0, 0.5, UnixNanos::from(2_000u64));
        assert_eq!(
            tracker.buy_overfill_bump(&vid),
            Some(Quantity::new(14.0, 6))
        );

        // Third partial crosses again: raise to cumulative 20.
        tracker.record_fill(&vid, 6.0, 0.5, UnixNanos::from(3_000u64));
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

        tracker.record_fill(&vid, snapped.as_f64(), 0.5, UnixNanos::from(1_000u64));
        assert!(tracker.buy_overfill_bump(&vid).is_none());
    }
}
