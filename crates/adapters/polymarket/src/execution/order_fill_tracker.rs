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
    identifiers::{AccountId, InstrumentId, TradeId, VenueOrderId},
    reports::FillReport,
    types::{Currency, Money, Price, Quantity},
};

use crate::common::consts::{SNAP_OVERFILL_ULPS, SNAP_UNDERFILL_ULPS};

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

/// Tracks per-order fill accumulation and detects dust residuals.
///
/// Thread-safe: internal `Mutex<FifoCacheMap>` -- safe to share via `Arc`
/// across the WS task and spawned order submission tasks.
#[derive(Debug)]
pub(crate) struct OrderFillTrackerMap {
    inner: Mutex<FifoCacheMap<VenueOrderId, OrderFillState, 10_000>>,
}

impl OrderFillTrackerMap {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(FifoCacheMap::default()),
        }
    }

    /// Register an order after HTTP accept.
    pub fn register(
        &self,
        venue_order_id: VenueOrderId,
        submitted_qty: Quantity,
        order_side: OrderSide,
        instrument_id: InstrumentId,
        size_precision: u8,
        price_precision: u8,
    ) {
        let state = OrderFillState {
            submitted_qty,
            cumulative_filled: 0.0,
            last_fill_px: 0.0,
            last_fill_ts: UnixNanos::default(),
            order_side,
            instrument_id,
            size_precision,
            price_precision,
        };
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .insert(venue_order_id, state);
    }

    /// Returns true if the order has been registered (accepted).
    pub fn contains(&self, venue_order_id: &VenueOrderId) -> bool {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .get(venue_order_id)
            .is_some()
    }

    /// Returns true if the order has received any fills or been removed (settled).
    pub fn has_fills_or_settled(&self, venue_order_id: &VenueOrderId) -> bool {
        let guard = self.inner.lock().expect(MUTEX_POISONED);
        match guard.get(venue_order_id) {
            Some(s) => s.cumulative_filled > 0.0,
            None => true, // Removed = already settled
        }
    }

    /// Returns the cumulative filled quantity for an order, if tracked.
    pub fn get_cumulative_filled(&self, venue_order_id: &VenueOrderId) -> Option<f64> {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .get(venue_order_id)
            .map(|s| s.cumulative_filled)
    }

    /// Returns `true` if cumulative fills have reached the submitted quantity
    /// (within a tight tolerance to account for f64 accumulation noise).
    pub fn is_fully_filled(&self, venue_order_id: &VenueOrderId) -> bool {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .get(venue_order_id)
            .is_some_and(|s| {
                let leaves = s.submitted_qty.as_f64() - s.cumulative_filled;
                leaves < 1e-9
            })
    }

    /// Record a fill, updating cumulative total and last price/ts.
    pub fn record_fill(&self, venue_order_id: &VenueOrderId, qty: f64, px: f64, ts: UnixNanos) {
        if let Some(s) = self
            .inner
            .lock()
            .expect(MUTEX_POISONED)
            .get_mut(venue_order_id)
        {
            s.cumulative_filled += qty;
            s.last_fill_px = px;
            s.last_fill_ts = ts;
        }
    }

    /// Snap a single fill qty to `submitted_qty` when the diff is dust.
    /// See `docs/integrations/polymarket.md` (Fill quantity normalization).
    pub fn snap_fill_qty(&self, venue_order_id: &VenueOrderId, fill_qty: Quantity) -> Quantity {
        let guard = self.inner.lock().expect(MUTEX_POISONED);
        match guard.get(venue_order_id) {
            Some(s) => {
                let diff = s.submitted_qty.as_f64() - fill_qty.as_f64();
                let ulp = 10f64.powi(-(s.size_precision as i32));
                let tolerance = if diff > 0.0 {
                    SNAP_UNDERFILL_ULPS * ulp
                } else if diff < 0.0 {
                    SNAP_OVERFILL_ULPS * ulp
                } else {
                    return fill_qty;
                };

                if diff.abs() < tolerance {
                    log::info!(
                        "Snapping fill qty {fill_qty} -> {} (dust={diff:.6})",
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

    /// Check if an order has a dust residual after all fills.
    /// Returns `Some(FillReport)` if a synthetic fill should be emitted.
    /// Removes the entry on dust settlement to prevent duplicate synthetic
    /// fills from repeated MATCHED events.
    #[expect(clippy::too_many_arguments)]
    pub fn check_dust_and_build_fill(
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
        let s = guard.get(venue_order_id)?;
        let leaves = s.submitted_qty.as_f64() - s.cumulative_filled;
        let ulp = 10f64.powi(-(s.size_precision as i32));
        let underfill_tolerance = SNAP_UNDERFILL_ULPS * ulp;

        if leaves > 0.0 && leaves < underfill_tolerance {
            // Copy fields before removing the entry
            let size_precision = s.size_precision;
            let price_precision = s.price_precision;
            let last_fill_px = s.last_fill_px;
            let order_side = s.order_side;
            let instrument_id = s.instrument_id;

            log::info!(
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
            guard.remove(venue_order_id);

            Some(FillReport {
                account_id,
                instrument_id,
                venue_order_id: *venue_order_id,
                trade_id,
                order_side,
                last_qty: dust_qty,
                last_px: fill_px,
                commission: Money::new(0.0, currency),
                liquidity_side: LiquiditySide::NoLiquiditySide,
                avg_px: None,
                report_id: UUID4::new(),
                ts_event,
                ts_init,
                client_order_id: None,
                venue_position_id: None,
            })
        } else {
            if leaves >= underfill_tolerance {
                log::info!(
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

    // Tolerances at size_precision=6:
    //   SNAP_UNDERFILL_ULPS = 10_000 -> 0.01
    //   SNAP_OVERFILL_ULPS  = 100    -> 0.0001
    #[rstest]
    // Underfill well within tolerance: CLOB truncated the fill to a cent
    // tick, snap UP to submitted_qty so the order can reach FILLED cleanly.
    #[case::underfill_dust(23.696681, 23.690000, 23.696681)]
    // Underfill near tolerance (0.0099 < 0.01): still snaps.
    #[case::underfill_near_tolerance(100.000000, 99.990100, 100.000000)]
    // Underfill at exactly the tolerance must NOT snap.
    #[case::underfill_at_tolerance(100.000000, 99.990000, 99.990000)]
    // Underfill above the tolerance: no snap.
    #[case::underfill_above_tolerance(100.000000, 99.980000, 99.980000)]
    // Underfill far past tolerance: leave fill alone.
    #[case::large_underfill(100.000000, 50.000000, 50.000000)]
    // Overfill within the tighter tolerance: V2 market BUY where the SDK
    // truncates registered qty to USDC scale but the on-chain fill comes
    // back at full precision. Observed drift is 4 ulps (4e-6 at
    // size_precision=6). Snap DOWN so the engine does not reject as
    // overfill.
    #[case::overfill_dust(714.285710, 714.285714, 714.285710)]
    // Overfill near the overfill tolerance (0.000099 < 0.0001): still snaps.
    #[case::overfill_near_tolerance(100.000000, 100.000099, 100.000000)]
    // Overfill at exactly the overfill tolerance must NOT snap.
    #[case::overfill_at_tolerance(100.000000, 100.000100, 100.000100)]
    // Overfill above the overfill tolerance but below the underfill
    // tolerance must NOT snap. This is the asymmetry: a 0.005 overfill is
    // suspicious and surfaces as an engine-side error rather than being
    // silently absorbed.
    #[case::overfill_above_tolerance(100.000000, 100.005000, 100.005000)]
    // Overfill far past tolerance: leave fill alone.
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

    // Tolerances scale with size_precision: at size_precision=3 the
    // underfill tolerance becomes 10 (10_000 * 1e-3) and the overfill
    // tolerance becomes 0.1 (100 * 1e-3). This case verifies the scaling.
    #[rstest]
    // Underfill within scaled tolerance (5 < 10): snap.
    #[case::underfill_scaled_within(100.000, 95.000, 100.000)]
    // Underfill above scaled tolerance (15 > 10): no snap.
    #[case::underfill_scaled_above(100.000, 85.000, 85.000)]
    // Overfill within scaled tolerance (0.05 < 0.1): snap.
    #[case::overfill_scaled_within(100.000, 100.050, 100.000)]
    // Overfill above scaled tolerance (0.2 > 0.1): no snap.
    #[case::overfill_scaled_above(100.000, 100.200, 100.200)]
    fn test_snap_fill_qty_scales_with_precision(
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
}
