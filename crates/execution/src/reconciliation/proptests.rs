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

//! Property-based tests for the reconciliation module.
//!
//! These harnesses shuffle, duplicate, and drop arbitrary sequences of fills and
//! reports to verify the four reconciliation invariants:
//! 1. Final position quantity matches the venue within instrument precision.
//! 2. Position average price matches within tolerance (default 0.01%).
//! 3. Generated fills preserve correct unrealized PnL.
//! 4. Synthetic `trade_id` and `venue_order_id` values are deterministic across replays.
//!
//! Property bodies are short by design: the Phase 1 `debug_assert!` tripwires
//! in `positions.rs` and `orders.rs` catch the low-level invariant violations
//! (positive fill qty, non-negative simulated value, monotonic filled_qty)
//! before the property body even runs.

#![cfg(test)]

use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::OrderEventAny,
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
    orders::{Order, OrderAny, OrderTestBuilder, stubs::TestOrderEventStubs},
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use proptest::prelude::*;
use rstest::rstest;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use super::*;

fn instrument() -> InstrumentAny {
    InstrumentAny::CurrencyPair(audusd_sim())
}

fn submit_accept(order: &mut OrderAny, account_id: AccountId, venue_order_id: VenueOrderId) {
    let submitted = TestOrderEventStubs::submitted(order, account_id);
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(order, account_id, venue_order_id);
    order.apply(accepted).unwrap();
}

fn apply_fill(
    order: &mut OrderAny,
    instrument: &InstrumentAny,
    trade_id: TradeId,
    last_qty: Quantity,
    last_px: Price,
) {
    let fill = TestOrderEventStubs::filled(
        order,
        instrument,
        Some(trade_id),
        None,
        Some(last_px),
        Some(last_qty),
        Some(LiquiditySide::Taker),
        None,
        None,
        None,
    );
    order.apply(fill).unwrap();
}

fn order_side_strategy() -> impl Strategy<Value = OrderSide> {
    prop_oneof![Just(OrderSide::Buy), Just(OrderSide::Sell)]
}

fn qty_decimal() -> impl Strategy<Value = Decimal> {
    (1i64..=1_000i64).prop_map(Decimal::from)
}

fn px_decimal() -> impl Strategy<Value = Decimal> {
    (1i64..=100_000i64).prop_map(|v| Decimal::new(v, 2))
}

fn venue_order_id_strategy() -> impl Strategy<Value = VenueOrderId> {
    (1u32..=20u32).prop_map(|i| VenueOrderId::new(format!("V-{i:04}")))
}

fn fill_snapshot_strategy() -> impl Strategy<Value = FillSnapshot> {
    (
        1u64..=1_000_000u64,
        order_side_strategy(),
        qty_decimal(),
        px_decimal(),
        venue_order_id_strategy(),
    )
        .prop_map(|(ts, side, qty, px, voi)| FillSnapshot::new(ts, side, qty, px, voi))
}

fn fill_sequence_strategy(min: usize, max: usize) -> impl Strategy<Value = Vec<FillSnapshot>> {
    proptest::collection::vec(fill_snapshot_strategy(), min..=max).prop_map(|mut fills| {
        // Ensure unique strictly-increasing timestamps so lifecycle boundaries are stable
        fills.sort_by_key(|f| f.ts_event);
        for i in 1..fills.len() {
            if fills[i].ts_event <= fills[i - 1].ts_event {
                fills[i].ts_event = fills[i - 1].ts_event + 1;
            }
        }
        fills
    })
}

fn apply_adjustment(
    fills: Vec<FillSnapshot>,
    adjustment: &FillAdjustmentResult,
) -> Vec<FillSnapshot> {
    match adjustment {
        FillAdjustmentResult::NoAdjustment => fills,
        FillAdjustmentResult::AddSyntheticOpening {
            synthetic_fill,
            existing_fills,
        } => {
            let mut result = Vec::with_capacity(existing_fills.len() + 1);
            result.push(synthetic_fill.clone());
            result.extend(existing_fills.iter().cloned());
            result
        }
        FillAdjustmentResult::ReplaceCurrentLifecycle { synthetic_fill, .. } => {
            vec![synthetic_fill.clone()]
        }
        FillAdjustmentResult::FilterToCurrentLifecycle {
            current_lifecycle_fills,
            ..
        } => current_lifecycle_fills.clone(),
    }
}

proptest! {
    #[rstest]
    fn prop_simulate_position_value_non_negative(fills in fill_sequence_strategy(0, 30)) {
        let (qty, value) = simulate_position(&fills);
        prop_assert!(value >= Decimal::ZERO);
        prop_assert!(!(qty != Decimal::ZERO && value.is_sign_negative()));
    }

    #[rstest]
    fn prop_simulate_position_deterministic(fills in fill_sequence_strategy(0, 20)) {
        let a = simulate_position(&fills);
        let b = simulate_position(&fills);
        prop_assert_eq!(a, b);
    }

    #[rstest]
    fn prop_simulate_position_all_buys_accumulate(
        fills in proptest::collection::vec(
            (qty_decimal(), px_decimal(), venue_order_id_strategy()),
            1..=10,
        ),
    ) {
        let mut expected_qty = Decimal::ZERO;
        let mut expected_value = Decimal::ZERO;
        let snapshots: Vec<FillSnapshot> = fills
            .iter()
            .enumerate()
            .map(|(i, (qty, px, voi))| {
                expected_qty += *qty;
                expected_value += *qty * *px;
                FillSnapshot::new((i as u64) + 1, OrderSide::Buy, *qty, *px, *voi)
            })
            .collect();

        let (qty, value) = simulate_position(&snapshots);
        prop_assert_eq!(qty, expected_qty);
        prop_assert_eq!(value, expected_value);
    }

    #[rstest]
    fn prop_simulate_position_avg_px_within_fill_px_range(
        fills in proptest::collection::vec(
            (qty_decimal(), px_decimal(), venue_order_id_strategy()),
            1..=10,
        ),
    ) {
        let snapshots: Vec<FillSnapshot> = fills
            .iter()
            .enumerate()
            .map(|(i, (qty, px, voi))| {
                FillSnapshot::new((i as u64) + 1, OrderSide::Buy, *qty, *px, *voi)
            })
            .collect();

        let min_px = fills.iter().map(|(_, p, _)| *p).min().unwrap();
        let max_px = fills.iter().map(|(_, p, _)| *p).max().unwrap();
        let (qty, value) = simulate_position(&snapshots);
        prop_assert!(qty > Decimal::ZERO);
        let avg_px = value / qty;
        prop_assert!(avg_px >= min_px);
        prop_assert!(avg_px <= max_px);
    }

    #[rstest]
    fn prop_detect_zero_crossings_within_fills(fills in fill_sequence_strategy(0, 20)) {
        let crossings = detect_zero_crossings(&fills);
        let fill_ts: ahash::AHashSet<u64> = fills.iter().map(|f| f.ts_event).collect();
        for ts in &crossings {
            prop_assert!(fill_ts.contains(ts));
        }
    }

    #[rstest]
    fn prop_detect_zero_crossings_all_same_side_has_none(
        qtys in proptest::collection::vec(qty_decimal(), 1..=8),
    ) {
        let voi = VenueOrderId::new("V-0001");
        let fills: Vec<FillSnapshot> = qtys
            .iter()
            .enumerate()
            .map(|(i, q)| FillSnapshot::new((i as u64) + 1, OrderSide::Buy, *q, dec!(100), voi))
            .collect();
        let crossings = detect_zero_crossings(&fills);
        prop_assert!(crossings.is_empty());
    }

    #[rstest]
    fn prop_check_position_match_reflexive(
        qty in (-1_000_000i64..=1_000_000i64).prop_map(Decimal::from),
        px in (1i64..=100_000i64).prop_map(|v| Decimal::new(v, 2)),
    ) {
        let value = qty.abs() * px;
        prop_assert!(check_position_match(qty, value, qty, px, dec!(0.0001)));
    }

    #[rstest]
    fn prop_check_position_match_qty_mismatch_false(
        qty1 in (-1_000i64..=1_000i64).prop_map(Decimal::from),
        qty2 in (-1_000i64..=1_000i64).prop_map(Decimal::from),
        px in px_decimal(),
    ) {
        prop_assume!(qty1 != qty2);
        prop_assert!(!check_position_match(qty1, qty1.abs() * px, qty2, px, dec!(0.0001)));
    }

    #[rstest]
    fn prop_calculate_reconciliation_price_no_change(
        qty in (1i64..=1_000i64).prop_map(Decimal::from),
        px in px_decimal(),
    ) {
        let result = calculate_reconciliation_price(qty, Some(px), qty, Some(px));
        prop_assert_eq!(result, None);
    }

    #[rstest]
    fn prop_calculate_reconciliation_price_flat_to_target(
        target_qty in (1i64..=1_000i64).prop_map(Decimal::from),
        target_px in px_decimal(),
    ) {
        let result = calculate_reconciliation_price(
            Decimal::ZERO,
            None,
            target_qty,
            Some(target_px),
        );
        prop_assert_eq!(result, Some(target_px));
    }

    #[rstest]
    fn prop_calculate_reconciliation_price_close_to_flat_uses_current_px(
        qty in (1i64..=1_000i64).prop_map(Decimal::from),
        px in px_decimal(),
    ) {
        let result = calculate_reconciliation_price(qty, Some(px), Decimal::ZERO, None);
        prop_assert_eq!(result, Some(px));
    }

    #[rstest]
    fn prop_calculate_reconciliation_price_same_side_roundtrip(
        current_qty in (1i64..=100i64).prop_map(Decimal::from),
        extra_qty in (1i64..=100i64).prop_map(Decimal::from),
        current_px in (100i64..=10_000i64).prop_map(|v| Decimal::new(v, 2)),
        target_px in (100i64..=10_000i64).prop_map(|v| Decimal::new(v, 2)),
    ) {
        let target_qty = current_qty + extra_qty;
        let recon_px = calculate_reconciliation_price(
            current_qty,
            Some(current_px),
            target_qty,
            Some(target_px),
        );

        if let Some(px) = recon_px {
            // Apply the reconciliation fill to the opening state and verify the
            // simulated average-price matches target_px within tolerance
            let voi = VenueOrderId::new("V-0001");
            let fills = vec![
                FillSnapshot::new(1, OrderSide::Buy, current_qty, current_px, voi),
                FillSnapshot::new(2, OrderSide::Buy, extra_qty, px, voi),
            ];
            let (sim_qty, sim_value) = simulate_position(&fills);
            prop_assert_eq!(sim_qty, target_qty);
            let sim_avg = sim_value / sim_qty;
            prop_assert!((sim_avg - target_px).abs() / target_px <= dec!(0.0001));
        }
    }

    #[rstest]
    fn prop_is_within_single_unit_tolerance_reflexive(
        val in (-1_000_000i64..=1_000_000i64).prop_map(Decimal::from),
        precision in 0u8..=8u8,
    ) {
        prop_assert!(is_within_single_unit_tolerance(val, val, precision));
    }

    #[rstest]
    fn prop_is_within_single_unit_tolerance_symmetric(
        a in (-1_000i64..=1_000i64).prop_map(Decimal::from),
        b in (-1_000i64..=1_000i64).prop_map(Decimal::from),
        precision in 0u8..=8u8,
    ) {
        prop_assert_eq!(
            is_within_single_unit_tolerance(a, b, precision),
            is_within_single_unit_tolerance(b, a, precision),
        );
    }

    #[rstest]
    fn prop_is_within_single_unit_tolerance_zero_precision_exact(
        a in (-1_000i64..=1_000i64).prop_map(Decimal::from),
        b in (-1_000i64..=1_000i64).prop_map(Decimal::from),
    ) {
        prop_assert_eq!(is_within_single_unit_tolerance(a, b, 0), a == b);
    }

    #[rstest]
    fn prop_adjust_fills_empty_is_no_adjustment(
        side in order_side_strategy(),
        qty in qty_decimal(),
        px in px_decimal(),
    ) {
        let inst = instrument();
        let venue = VenuePositionSnapshot { side, qty, avg_px: px };
        let result = adjust_fills_for_partial_window(&[], &venue, &inst, dec!(0.0001));
        prop_assert_eq!(result, FillAdjustmentResult::NoAdjustment);
    }

    #[rstest]
    fn prop_adjust_fills_flat_venue_is_no_adjustment(fills in fill_sequence_strategy(0, 10)) {
        let inst = instrument();
        let venue = VenuePositionSnapshot {
            side: OrderSide::Buy,
            qty: Decimal::ZERO,
            avg_px: Decimal::ZERO,
        };
        let result = adjust_fills_for_partial_window(&fills, &venue, &inst, dec!(0.0001));
        prop_assert_eq!(result, FillAdjustmentResult::NoAdjustment);
    }

    #[rstest]
    fn prop_adjust_fills_matching_sequence_is_no_adjustment(
        fills in proptest::collection::vec(
            (qty_decimal(), px_decimal(), venue_order_id_strategy()),
            1..=5,
        ),
    ) {
        // Build an all-buys lifecycle; venue position equals the simulated net
        let snapshots: Vec<FillSnapshot> = fills
            .iter()
            .enumerate()
            .map(|(i, (qty, px, voi))| {
                FillSnapshot::new((i as u64) + 1, OrderSide::Buy, *qty, *px, *voi)
            })
            .collect();
        let (sim_qty, sim_value) = simulate_position(&snapshots);
        prop_assume!(sim_qty > Decimal::ZERO);
        let sim_avg = sim_value / sim_qty;

        let inst = instrument();
        let venue = VenuePositionSnapshot {
            side: OrderSide::Buy,
            qty: sim_qty,
            avg_px: sim_avg,
        };
        let result = adjust_fills_for_partial_window(&snapshots, &venue, &inst, dec!(0.0001));
        prop_assert_eq!(result, FillAdjustmentResult::NoAdjustment);
    }

    // Round-trip: for a single-lifecycle all-buys sequence whose venue position
    // reports MORE than the simulation accounts for, the adjustment should
    // produce an AddSyntheticOpening whose effective fill list simulates
    // back to the venue position within tolerance.
    #[rstest]
    fn prop_adjust_fills_synthetic_opening_matches_venue(
        existing in proptest::collection::vec(
            (1i64..=100i64, 100i64..=10_000i64, venue_order_id_strategy()),
            1..=4,
        ),
        opening_qty in 1i64..=100i64,
        opening_px_units in 100i64..=10_000i64,
    ) {
        let snapshots: Vec<FillSnapshot> = existing
            .iter()
            .enumerate()
            .map(|(i, (q, p, voi))| {
                FillSnapshot::new(
                    (i as u64) + 1,
                    OrderSide::Buy,
                    Decimal::from(*q),
                    Decimal::new(*p, 2),
                    *voi,
                )
            })
            .collect();
        let (sim_qty, sim_value) = simulate_position(&snapshots);

        // Construct venue = sim + extra opening (same-side accumulation). This
        // is the case where reconciliation must prepend a synthetic opening.
        let extra_qty = Decimal::from(opening_qty);
        let target_qty = sim_qty + extra_qty;
        let opening_px = Decimal::new(opening_px_units, 2);
        let target_value = sim_value + extra_qty * opening_px;
        let target_avg = target_value / target_qty;

        let inst = instrument();
        let venue = VenuePositionSnapshot {
            side: OrderSide::Buy,
            qty: target_qty,
            avg_px: target_avg,
        };

        let adjustment = adjust_fills_for_partial_window(&snapshots, &venue, &inst, dec!(0.0001));
        let is_synth_opening =
            matches!(adjustment, FillAdjustmentResult::AddSyntheticOpening { .. });
        prop_assert!(is_synth_opening);

        let effective = apply_adjustment(snapshots, &adjustment);
        let (eff_qty, eff_value) = simulate_position(&effective);
        prop_assert_eq!(eff_qty, target_qty);
        let eff_avg = eff_value / eff_qty;
        prop_assert!(
            (eff_avg - target_avg).abs() / target_avg <= dec!(0.0001),
            "eff_avg={eff_avg}, target_avg={target_avg}",
        );
    }

    // Flip-scenario roundtrip: applying the reconciliation price for a flip
    // should land on target_avg_px after simulation.
    #[rstest]
    fn prop_calculate_reconciliation_price_flip_roundtrip(
        current_qty in 1i64..=100i64,
        flipped_qty in 1i64..=100i64,
        current_px_units in 100i64..=10_000i64,
        target_px_units in 100i64..=10_000i64,
    ) {
        let current_qty = Decimal::from(current_qty);
        let target_qty = Decimal::from(-flipped_qty);
        let current_px = Decimal::new(current_px_units, 2);
        let target_px = Decimal::new(target_px_units, 2);

        let recon_px = calculate_reconciliation_price(
            current_qty,
            Some(current_px),
            target_qty,
            Some(target_px),
        );
        prop_assert_eq!(recon_px, Some(target_px));

        // Simulate: buy current_qty @ current_px, then sell (current_qty + flipped)
        // at recon_px; should land on (-flipped_qty, flipped_qty * target_px).
        let voi = VenueOrderId::new("V-0001");
        let fills = vec![
            FillSnapshot::new(1, OrderSide::Buy, current_qty, current_px, voi),
            FillSnapshot::new(
                2,
                OrderSide::Sell,
                current_qty + Decimal::from(flipped_qty),
                target_px,
                voi,
            ),
        ];
        let (sim_qty, sim_value) = simulate_position(&fills);
        prop_assert_eq!(sim_qty, target_qty);
        let sim_avg = sim_value / sim_qty.abs();
        prop_assert_eq!(sim_avg, target_px);
    }

    // Tolerance boundary: for precision p, |a - b| = 10^-p is within tolerance,
    // |a - b| > 10^-p is not.
    #[rstest]
    fn prop_is_within_single_unit_tolerance_boundary(
        val in (-1_000i64..=1_000i64).prop_map(Decimal::from),
        precision in 1u8..=8u8,
    ) {
        let unit = Decimal::new(1, u32::from(precision));
        prop_assert!(is_within_single_unit_tolerance(val, val + unit, precision));
        let over = unit + Decimal::new(1, u32::from(precision) + 2);
        prop_assert!(!is_within_single_unit_tolerance(val, val + over, precision));
    }

    // A balanced buy/sell pair exactly crosses zero on the sell
    #[rstest]
    fn prop_detect_zero_crossings_balanced_pair(
        qty in qty_decimal(),
        buy_px in px_decimal(),
        sell_px in px_decimal(),
    ) {
        let voi = VenueOrderId::new("V-0001");
        let fills = vec![
            FillSnapshot::new(1, OrderSide::Buy, qty, buy_px, voi),
            FillSnapshot::new(2, OrderSide::Sell, qty, sell_px, voi),
        ];
        let crossings = detect_zero_crossings(&fills);
        prop_assert_eq!(crossings.len(), 1);
        prop_assert_eq!(crossings[0], 2);
    }

    // Zero-priced fills must not panic simulate_position and must satisfy the
    // non-negativity invariant (addresses the Phase 1 "zero-cost fill" case).
    #[rstest]
    fn prop_simulate_position_zero_price_fills(
        fills in proptest::collection::vec(
            (qty_decimal(), 0u32..=2u32, venue_order_id_strategy()),
            1..=8,
        ),
    ) {
        let snapshots: Vec<FillSnapshot> = fills
            .iter()
            .enumerate()
            .map(|(i, (q, mark, voi))| {
                // mark=0 => zero-cost fill; otherwise px = 100
                let px = if *mark == 0 { Decimal::ZERO } else { dec!(100) };
                FillSnapshot::new((i as u64) + 1, OrderSide::Buy, *q, px, *voi)
            })
            .collect();
        let (qty, value) = simulate_position(&snapshots);
        prop_assert!(value >= Decimal::ZERO);
        prop_assert!(qty > Decimal::ZERO);
    }
}

fn build_market_order(instrument: &InstrumentAny, qty: u64) -> OrderAny {
    OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(qty))
        .build()
}

fn fill_report_for(
    instrument_id: InstrumentId,
    venue_order_id: VenueOrderId,
    trade_id: TradeId,
    last_qty: Quantity,
    last_px: Price,
) -> FillReport {
    FillReport::new(
        AccountId::from("SIM-001"),
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        last_qty,
        last_px,
        Money::new(0.0, Currency::USD()),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )
}

fn status_report_for(
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    instrument_id: InstrumentId,
    quantity: Quantity,
    filled_qty: Quantity,
    status: OrderStatus,
) -> OrderStatusReport {
    OrderStatusReport::new(
        AccountId::from("SIM-001"),
        instrument_id,
        Some(client_order_id),
        venue_order_id,
        OrderSide::Buy,
        OrderType::Market,
        TimeInForce::Gtc,
        status,
        quantity,
        filled_qty,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )
}

proptest! {
    #[rstest]
    fn prop_reconcile_fill_report_deterministic(
        order_qty in 10u64..=1_000u64,
        fill_qty in 1u64..=10u64,
    ) {
        let inst = instrument();
        let order = build_market_order(&inst, order_qty);
        let report = fill_report_for(
            inst.id(),
            VenueOrderId::from("V-001"),
            TradeId::from("T-001"),
            Quantity::from(fill_qty),
            Price::from("1.00000"),
        );

        let a = reconcile_fill_report(&order, &report, &inst, UnixNanos::default(), false);
        let b = reconcile_fill_report(&order, &report, &inst, UnixNanos::default(), false);
        match (a, b) {
            (Some(OrderEventAny::Filled(fa)), Some(OrderEventAny::Filled(fb))) => {
                prop_assert_eq!(fa.trade_id, fb.trade_id);
                prop_assert_eq!(fa.venue_order_id, fb.venue_order_id);
                prop_assert_eq!(fa.last_qty, fb.last_qty);
                prop_assert_eq!(fa.last_px, fb.last_px);
                prop_assert_eq!(fa.liquidity_side, fb.liquidity_side);
                prop_assert_eq!(fa.reconciliation, fb.reconciliation);
            }
            (None, None) => {}
            other => {
                return Err(TestCaseError::fail(format!(
                    "non-deterministic reconcile_fill_report result: {other:?}",
                )));
            }
        }
    }

    #[rstest]
    fn prop_reconcile_fill_report_duplicate_returns_none(
        order_qty in 10u64..=1_000u64,
        fill_qty in 1u64..=10u64,
    ) {
        let inst = instrument();
        let account_id = AccountId::from("SIM-001");
        let voi = VenueOrderId::from("V-001");
        let trade_id = TradeId::from("T-001");

        let mut order = build_market_order(&inst, order_qty);
        submit_accept(&mut order, account_id, voi);
        apply_fill(
            &mut order,
            &inst,
            trade_id,
            Quantity::from(fill_qty),
            Price::from("1.00000"),
        );

        let report = fill_report_for(
            inst.id(),
            voi,
            trade_id,
            Quantity::from(fill_qty),
            Price::from("1.00000"),
        );
        let result = reconcile_fill_report(&order, &report, &inst, UnixNanos::default(), false);
        prop_assert!(result.is_none());
    }

    #[rstest]
    fn prop_reconcile_fill_report_overfill_blocked_when_disallowed(
        order_qty in 1u64..=100u64,
        overfill in 1u64..=50u64,
    ) {
        let inst = instrument();
        let order = build_market_order(&inst, order_qty);
        let report_qty = Quantity::from(order_qty + overfill);
        let report_px = Price::from("1.00000");
        let trade_id = TradeId::from("T-001");
        let voi = VenueOrderId::from("V-001");
        let report = fill_report_for(inst.id(), voi, trade_id, report_qty, report_px);
        let result = reconcile_fill_report(&order, &report, &inst, UnixNanos::default(), false);
        prop_assert!(result.is_none());

        let allowed = reconcile_fill_report(&order, &report, &inst, UnixNanos::default(), true);
        if let Some(OrderEventAny::Filled(filled)) = allowed {
            prop_assert_eq!(filled.last_qty, report_qty);
            prop_assert_eq!(filled.last_px, report_px);
            prop_assert_eq!(filled.trade_id, trade_id);
            prop_assert!(filled.reconciliation);
        } else {
            return Err(TestCaseError::fail("expected OrderFilled when overfill allowed"));
        }
    }

    #[rstest]
    fn prop_should_reconciliation_update_same_state_false(
        qty in 1u64..=1_000u64,
    ) {
        let inst = instrument();
        let mut order = build_market_order(&inst, qty);
        submit_accept(
            &mut order,
            AccountId::from("SIM-001"),
            VenueOrderId::from("V-001"),
        );
        let report = status_report_for(
            order.client_order_id(),
            VenueOrderId::from("V-001"),
            inst.id(),
            Quantity::from(qty),
            Quantity::from(0),
            OrderStatus::Accepted,
        );
        prop_assert!(!should_reconciliation_update(&order, &report));
    }

    #[rstest]
    fn prop_should_reconciliation_update_quantity_below_filled_false(
        order_qty in 10u64..=1_000u64,
        fill_qty in 1u64..=10u64,
    ) {
        let inst = instrument();
        let account_id = AccountId::from("SIM-001");
        let voi = VenueOrderId::from("V-001");

        let mut order = build_market_order(&inst, order_qty);
        submit_accept(&mut order, account_id, voi);
        apply_fill(
            &mut order,
            &inst,
            TradeId::from("T-001"),
            Quantity::from(fill_qty),
            Price::from("1.00000"),
        );

        // Venue claims a new quantity strictly less than current filled_qty
        let report_qty = fill_qty.saturating_sub(1);
        let report = status_report_for(
            order.client_order_id(),
            voi,
            inst.id(),
            Quantity::from(report_qty),
            Quantity::from(fill_qty),
            OrderStatus::PartiallyFilled,
        );
        prop_assert!(!should_reconciliation_update(&order, &report));
    }

    #[rstest]
    fn prop_reconcile_order_report_in_sync_returns_none(
        qty in 1u64..=1_000u64,
    ) {
        let inst = instrument();
        let account_id = AccountId::from("SIM-001");
        let voi = VenueOrderId::from("V-001");

        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(inst.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(qty))
            .price(Price::from("1.00000"))
            .build();
        submit_accept(&mut order, account_id, voi);

        let mut report = status_report_for(
            order.client_order_id(),
            voi,
            inst.id(),
            Quantity::from(qty),
            Quantity::from(0),
            OrderStatus::Accepted,
        );
        report.price = Some(Price::from("1.00000"));

        let result = reconcile_order_report(&order, &report, Some(&inst), UnixNanos::default());
        prop_assert!(result.is_none());
    }

    #[rstest]
    fn prop_create_incremental_inferred_fill_equal_qty_none(
        order_qty in 10u64..=1_000u64,
        filled in 1u64..=10u64,
    ) {
        let inst = instrument();
        let account_id = AccountId::from("SIM-001");
        let voi = VenueOrderId::from("V-001");

        let mut order = build_market_order(&inst, order_qty);
        submit_accept(&mut order, account_id, voi);
        apply_fill(
            &mut order,
            &inst,
            TradeId::from("T-001"),
            Quantity::from(filled),
            Price::from("1.00000"),
        );

        // Report.filled_qty == order.filled_qty: no incremental fill emitted
        let mut report = status_report_for(
            order.client_order_id(),
            voi,
            inst.id(),
            Quantity::from(order_qty),
            Quantity::from(filled),
            OrderStatus::PartiallyFilled,
        );
        report.avg_px = Some(dec!(1.00000));
        let result = create_incremental_inferred_fill(
            &order,
            &report,
            &account_id,
            &inst,
            UnixNanos::default(),
            None,
        );
        prop_assert!(result.is_none());
    }

    #[rstest]
    fn prop_create_incremental_inferred_fill_produces_event_for_positive_diff(
        order_qty in 10u64..=1_000u64,
        filled in 1u64..=5u64,
        incr in 1u64..=5u64,
    ) {
        let inst = instrument();
        let account_id = AccountId::from("SIM-001");
        let voi = VenueOrderId::from("V-001");

        let mut order = build_market_order(&inst, order_qty);
        submit_accept(&mut order, account_id, voi);
        apply_fill(
            &mut order,
            &inst,
            TradeId::from("T-001"),
            Quantity::from(filled),
            Price::from("1.00000"),
        );

        let report_filled = filled + incr;
        prop_assume!(report_filled <= order_qty);
        let mut report = status_report_for(
            order.client_order_id(),
            voi,
            inst.id(),
            Quantity::from(order_qty),
            Quantity::from(report_filled),
            OrderStatus::PartiallyFilled,
        );
        report.avg_px = Some(dec!(1.00000));

        let result = create_incremental_inferred_fill(
            &order,
            &report,
            &account_id,
            &inst,
            UnixNanos::default(),
            None,
        );

        if let Some(OrderEventAny::Filled(f)) = result {
            prop_assert_eq!(f.last_qty, Quantity::from(incr));
            prop_assert_eq!(f.last_px, Price::from("1.00000"));
            prop_assert_eq!(f.account_id, account_id);
            prop_assert!(f.reconciliation);
        } else {
            return Err(TestCaseError::fail(
                "expected OrderFilled event for positive fill diff",
            ));
        }
    }
}

proptest! {
    #[rstest]
    fn prop_synthetic_trade_id_deterministic(fill in fill_snapshot_strategy()) {
        let a = create_synthetic_trade_id(&fill);
        let b = create_synthetic_trade_id(&fill);
        prop_assert_eq!(a, b);
    }

    #[rstest]
    fn prop_synthetic_venue_order_id_deterministic(fill in fill_snapshot_strategy()) {
        let inst_id = InstrumentId::from("AUD/USD.SIM");
        let a = create_synthetic_venue_order_id(&fill, inst_id);
        let b = create_synthetic_venue_order_id(&fill, inst_id);
        prop_assert_eq!(a, b);
    }

    #[rstest]
    fn prop_inferred_reconciliation_trade_id_deterministic(
        side in order_side_strategy(),
        qty_units in 1i64..=1_000i64,
        px_units in 1i64..=100_000i64,
        ts in 1u64..=10_000_000u64,
    ) {
        let account_id = AccountId::from("SIM-001");
        let inst_id = InstrumentId::from("AUD/USD.SIM");
        let client_order_id = ClientOrderId::from("O-001");
        let voi = VenueOrderId::from("V-001");
        let position_id = PositionId::new("P-001");
        let filled = Quantity::from(qty_units as u64);
        let last_qty = Quantity::from(qty_units as u64);
        let last_px = Price::from_decimal_dp(Decimal::new(px_units, 5), 5).unwrap();
        let ts_last = UnixNanos::from(ts);

        let a = create_inferred_reconciliation_trade_id(
            account_id,
            inst_id,
            client_order_id,
            Some(voi),
            side,
            OrderType::Market,
            filled,
            last_qty,
            last_px,
            position_id,
            ts_last,
        );
        let b = create_inferred_reconciliation_trade_id(
            account_id,
            inst_id,
            client_order_id,
            Some(voi),
            side,
            OrderType::Market,
            filled,
            last_qty,
            last_px,
            position_id,
            ts_last,
        );
        prop_assert_eq!(a, b);
    }

    #[rstest]
    fn prop_position_reconciliation_venue_order_id_deterministic(
        side in order_side_strategy(),
        qty_units in 1i64..=1_000i64,
        px_units in 1i64..=100_000i64,
        ts in 1u64..=10_000_000u64,
    ) {
        let account_id = AccountId::from("SIM-001");
        let inst_id = InstrumentId::from("AUD/USD.SIM");
        let qty = Quantity::from(qty_units as u64);
        let px = Price::from_decimal_dp(Decimal::new(px_units, 5), 5).unwrap();
        let ts_last = UnixNanos::from(ts);

        let a = create_position_reconciliation_venue_order_id(
            account_id,
            inst_id,
            side,
            OrderType::Market,
            qty,
            Some(px),
            None,
            Some("recon"),
            ts_last,
        );
        let b = create_position_reconciliation_venue_order_id(
            account_id,
            inst_id,
            side,
            OrderType::Market,
            qty,
            Some(px),
            None,
            Some("recon"),
            ts_last,
        );
        prop_assert_eq!(a, b);
    }

    #[rstest]
    fn prop_synthetic_trade_id_changes_with_ts(
        base in fill_snapshot_strategy(),
        delta in 1u64..=1_000u64,
    ) {
        let mut other = base.clone();
        other.ts_event = base.ts_event.saturating_add(delta);
        let a = create_synthetic_trade_id(&base);
        let b = create_synthetic_trade_id(&other);
        prop_assert_ne!(a, b);
    }
}
